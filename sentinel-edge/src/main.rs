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
mod logbuf;
mod state;
mod tailscale;

use state::AppState;

fn main() {
    // Capture logs into a shared ring (also teed to stderr) so the box can push them up
    // for the in-UI log viewer — the edge is no longer a black box.
    let logs = logbuf::init();

    let bind = std::env::var("EDGE_BIND").unwrap_or_else(|_| "0.0.0.0:8788".to_string());

    // Join the tailnet + expose go2rtc over it (opt-in via TS_AUTHKEY) FIRST, so the
    // detected EDGE_PUBLIC_HOST is reported to control on the very first camera pull.
    tailscale::ensure_up();

    // Config-light boot: pull the camera list from the control plane (cache fallback).
    // No database — the appliance carries no Postgres.
    let state = AppState::boot();

    // go2rtc shells out to ffmpeg for JPEG frames (snapshots + the GetCurrentFrame node).
    // Surface a missing ffmpeg loudly at boot — it now also rides the cloud log viewer.
    if std::process::Command::new("ffmpeg").arg("-version").output().map(|o| o.status.success()).unwrap_or(false) {
        log::info!("ffmpeg found on PATH");
    } else {
        log::warn!(
            "ffmpeg NOT found on PATH — go2rtc JPEG frames (snapshots / GetCurrentFrame) will fail. \
             Install it (Linux: apt install ffmpeg · macOS: brew install ffmpeg)."
        );
    }

    if state.own_go2rtc {
        go2rtc::respawn(&state);
        log::info!("edge owns go2rtc (EDGE_OWN_GO2RTC)");
    } else {
        log::info!("edge using external go2rtc on :1984 (+ ONVIF fallback)");
    }

    // Discovery loop: sweep the LAN for ONVIF cameras every 5 min — reconciling any camera
    // that changed IP (matched by stable ONVIF id) and pushing the sweep to control so the
    // cloud Devices page can surface them. Moves self-heal here even without a manual rescan.
    {
        let disco = std::sync::Arc::clone(&state);
        std::thread::spawn(move || loop {
            disco.rediscover();
            std::thread::sleep(std::time::Duration::from_secs(300));
        });
    }

    // Config long-poll: block on control until the web changes a camera (or 25s), then
    // apply immediately — so cloud edits (e.g. setting credentials) take effect in ~1s.
    // After each cycle, check for a pending rescan command (the UI's "Rediscover" button)
    // and run it now (woken via the same long-poll bump).
    {
        let sync = std::sync::Arc::clone(&state);
        std::thread::spawn(move || loop {
            sync.reload(25);
            if sync.check_rescan_command() {
                log::info!("rescan requested from the cloud — running discovery now");
                sync.rediscover();
            }
        });
    }

    // Log push loop: ship new log lines (edge + go2rtc) up to control every few seconds so
    // the cloud log viewer shows a near-live tail. Best-effort; stdout remains the source.
    {
        let logs_state = std::sync::Arc::clone(&state);
        std::thread::spawn(move || {
            let mut cursor = 0u64;
            loop {
                std::thread::sleep(std::time::Duration::from_secs(3));
                let (lines, next) = logs.since(cursor);
                if !lines.is_empty() && logs_state.push_logs(&lines) {
                    cursor = next;
                }
            }
        });
    }

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
