//! `sentinel-edge` — the edge gateway.
//!
//! Phase 2 of the cloud decomposition: the **only** thing that touches cameras. It
//! wraps go2rtc (`sentinel-rtc`) + ONVIF and serves on-demand JPEG snapshots over
//! REST, so out-of-process services reach the LAN-bound cameras without speaking
//! RTSP/ONVIF themselves. `sentinel-flow`'s `FrameSource` becomes a client of this
//! (`EDGE_URL`), fetching binary frames instead of base64'ing them onto SSE.
//!
//! Cameras are LAN-bound (ONVIF 192.168.x.x; go2rtc binds 1984/8554/8555), so an
//! edge tier is mandatory however cloud the rest becomes.
//!
//! Run:
//!   DATABASE_URL=postgres://sentinel:sentinel@localhost/sentinel \
//!   EDGE_BIND=0.0.0.0:8788 cargo run -p sentinel-edge   # or: task edge
//!
//! Optional env: `EDGE_OWN_GO2RTC=1` to spawn/own the go2rtc sidecar (set
//! `GO2RTC_BIN` + `GO2RTC_CONFIG`); otherwise it uses whatever go2rtc is on :1984.
mod go2rtc;
mod http;
mod state;
mod tailscale;

use state::AppState;

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let bind = std::env::var("EDGE_BIND").unwrap_or_else(|_| "0.0.0.0:8788".to_string());

    // Config-light boot: pull the camera list from the control plane (cache fallback).
    // No database — the appliance carries no Postgres.
    let state = AppState::boot();

    if state.own_go2rtc {
        go2rtc::respawn(&state);
        log::info!("edge owns go2rtc (EDGE_OWN_GO2RTC)");
    } else {
        log::info!("edge using external go2rtc on :1984 (+ ONVIF fallback)");
    }

    // Join the tailnet + expose go2rtc over it (opt-in via TS_AUTHKEY).
    tailscale::ensure_up();

    let rt = tokio::runtime::Runtime::new().expect("build tokio runtime");
    rt.block_on(async move {
        let app = http::router(state);
        let listener = match tokio::net::TcpListener::bind(&bind).await {
            Ok(l) => l,
            Err(e) => {
                log::error!("failed to bind {bind}: {e}");
                std::process::exit(1);
            }
        };
        log::info!("sentinel-edge listening on http://{bind} (GET /snapshot/<ip>)");
        if let Err(e) = axum::serve(listener, app).with_graceful_shutdown(shutdown_signal()).await {
            log::error!("server error: {e}");
            std::process::exit(1);
        }
    });
    log::info!("sentinel-edge shut down");
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
