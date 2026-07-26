//! `sentinel-gateway` — the public streaming bridge.
//!
//! Phase 2 of the cloud decomposition. End-user browsers connect here; the gateway
//! proxies live video (MSE over WebSocket) and JPEG snapshots through to a client's
//! on-prem `sentinel-edge` go2rtc over the Tailscale tailnet. The edge's go2rtc is
//! localhost-only + `tailscale serve`'d, so it's never exposed directly.
//!
//! Single-tenant for now: one configured upstream (`EDGE_UPSTREAM`). The per-tenant
//! registry + JWT slot into `AppState::resolve_upstream` later (no transport change).
//!
//! Run:
//!   EDGE_UPSTREAM=http://127.0.0.1:1984 GATEWAY_BIND=0.0.0.0:8789 \
//!   cargo run -p sentinel-gateway        # or: task gateway
//!
//! Optional env: `TS_AUTHKEY`/`TS_HOSTNAME`/`TS_TAGS` to join the tailnet at startup.
mod auth;
mod http;
mod proxy;
mod state;
mod tailscale;

use std::sync::Arc;

use state::AppState;

#[tokio::main]
async fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

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
    let jwt_secret = std::env::var("JWT_SECRET").unwrap_or_else(|_| {
        log::warn!("JWT_SECRET unset — using an insecure dev secret (must match control)");
        "dev-insecure-secret-change-me".to_string()
    });

    // Join the tailnet if configured (so MagicDNS upstreams resolve over the VPN).
    tailscale::ensure_up();

    // This gateway's public WS base — the address an edge dials back for live-over-outbound
    // (card #144). Unset → outbound live is off and live uses the tailnet path.
    let public_ws_url = std::env::var("GATEWAY_PUBLIC_WS").unwrap_or_default();

    let state = Arc::new(AppState::new(upstream.clone(), control_url.clone(), jwt_secret, public_ws_url.clone()));
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

    if let Err(e) = axum::serve(listener, app).with_graceful_shutdown(shutdown_signal()).await {
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
        if let Ok(mut s) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
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
