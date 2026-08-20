//! `sentinel-gateway` — the public streaming bridge.
//!
//! Phase 2 of the cloud decomposition. End-user browsers connect here; the gateway
//! proxies live video (MSE over WebSocket) and JPEG snapshots through to a client's
//! localhost-only behind the reverse proxy, so it's never exposed directly.
//!
//! Single-tenant for now: one configured upstream (`EDGE_UPSTREAM`). The per-tenant
//! registry + JWT slot into `AppState::resolve_upstream` later (no transport change).
//!
//! Run:
//!   EDGE_UPSTREAM=http://127.0.0.1:1984 GATEWAY_BIND=0.0.0.0:8789 \
//!   cargo run -p sentinel-gateway        # or: task gateway
//!
mod auth;
mod datadog;
mod http;
mod proxy;
mod state;

use std::sync::Arc;

use state::AppState;

#[tokio::main]
async fn main() {
    // Card #146: structured logs → Datadog (`service:gateway`) + `log::*` bridge, replacing
    // env_logger. Must run inside the tokio runtime — the Datadog drain task is spawned lazily.
    datadog::init_tracing();

    let bind = std::env::var("GATEWAY_BIND").unwrap_or_else(|_| "0.0.0.0:8789".to_string());
    let upstream = std::env::var("EDGE_UPSTREAM")
        .ok()
        .map(|s| s.trim().trim_end_matches('/').to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "http://127.0.0.1:1984".to_string());
    // Control plane base for the per-tenant box-host lookup (/internal/box-upstream).
    let control_url = std::env::var("CONTROL_URL")
        .ok()
        .map(|s| s.trim().trim_end_matches('/').to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "http://127.0.0.1:8790".to_string());

    // Shared with sentinel-control: the live endpoints require a JWT carrying
    // `live.view`. Falls back to the same insecure dev default control uses so a local
    // end-to-end setup works; both MUST set a real JWT_SECRET in production.
    // Must resolve identically to control: a mismatch here does not fail loudly, it silently
    // rejects every live-video token. Same fail-closed rule — a release build never invents one.
    let jwt_secret = match std::env::var("JWT_SECRET").ok() {
        Some(s) if s.trim().len() >= 32 => s,
        other => {
            if cfg!(debug_assertions) {
                log::warn!("JWT_SECRET unset or too short — using the insecure dev secret (debug build only)");
                "dev-insecure-secret-change-me".to_string()
            } else {
                log::error!(
                    "refusing to start: JWT_SECRET {} (it must match control's)",
                    if other.is_none() {
                        "is unset"
                    } else {
                        "is shorter than 32 characters"
                    }
                );
                std::process::exit(1);
            }
        }
    };

    // This gateway's public WS base — the address an edge dials back for live-over-outbound
    // (card #144). Unset → outbound live is off and live uses the direct path.
    let public_ws_url = std::env::var("GATEWAY_PUBLIC_WS").unwrap_or_default();

    let state = Arc::new(AppState::new(
        upstream.clone(),
        control_url.clone(),
        jwt_secret,
        public_ws_url.clone(),
    ));
    log::info!("sentinel-gateway upstream = {upstream} (fallback), control = {control_url}, public_ws = {}", if public_ws_url.is_empty() { "(unset — outbound live off)" } else { &public_ws_url });

    let app = http::router(state);
    let listener = match tokio::net::TcpListener::bind(&bind).await {
        Ok(l) => l,
        Err(e) => {
            log::error!("failed to bind {bind}: {e}");
            std::process::exit(1);
        }
    };
    log::info!("sentinel-gateway listening on http://{bind} (MSE /api/ws, snapshots /api/frame.jpeg, /player)");

    if let Err(e) = axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
    {
        log::error!("server error: {e}");
        std::process::exit(1);
    }
    log::info!("sentinel-gateway shut down");
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let term = async {
        if let Ok(mut s) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            s.recv().await;
        }
    };
    #[cfg(not(unix))]
    let term = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = term => {},
    }
    log::info!("shutdown signal received");
}
