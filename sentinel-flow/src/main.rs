//! `sentinel-flow` — the always-on workflow engine service.
//!
//! Phase 1 of the cloud decomposition: the workflow engine + trigger poller + run
//! executor move OUT of the Tauri process into this standalone service, so the
//! UI/control-plane can be stopped and restarted freely while automations keep
//! firing 24/7. It links the same UI-agnostic `app-core` (engine, nodes, repos,
//! Phase 0.6 ports), connects to the shared Postgres, and exposes a REST + SSE
//! control plane the UI consumes.
//!
//! Run:
//!   DATABASE_URL=postgres://sentinel:sentinel@localhost/sentinel \
//!   FLOW_BIND=0.0.0.0:8787 cargo run -p sentinel-flow
//!
//! Optional env: `SENTINEL_MODELS_DIR` (face/vision models), `TELEGRAM_BOT_TOKEN`
//! / `TELEGRAM_PAIRING_CODE` (notification fallback when not set in Postgres).
mod bus;
mod engine;
mod events;
mod http;
mod state;

use std::time::{SystemTime, UNIX_EPOCH};

use state::AppState;

/// Unix epoch millis now — the wall-clock basis for persisted cron timers (shared
/// by the engine restore + the poller, mirroring the Tauri shell).
pub fn now_epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let database_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| {
        "postgres://sentinel:sentinel@localhost/sentinel".to_string()
    });
    let bind = std::env::var("FLOW_BIND").unwrap_or_else(|_| "0.0.0.0:8787".to_string());

    // Boot SYNCHRONOUSLY, before any tokio runtime exists: `Db::connect` drives its
    // own runtime with `block_on`, which would panic ("runtime within a runtime") if
    // called from inside the server's async context. Retry so the service survives
    // Postgres coming up after it (e.g. compose ordering).
    let state = boot_with_retry(&database_url);

    // The engine + poller run on their own std threads (blocking), like the Tauri
    // shell. `Db::block` bridges sqlx from there safely (it spawns, never block_on).
    engine::spawn_poller(state.clone());
    log::info!("Workflow engine + trigger poller started");

    // Now build the tokio runtime for the HTTP control plane.
    let rt = tokio::runtime::Runtime::new().expect("build tokio runtime");
    rt.block_on(async move {
        // Bridge this pod's events onto the Redis fabric (multi-pod SSE relay) + consume
        // externally-injected events (edge motion via control) into the engine.
        bus::spawn_republisher(state.clone());
        bus::spawn_injector(state.clone());
        let app = http::router(state);
        let listener = match tokio::net::TcpListener::bind(&bind).await {
            Ok(l) => l,
            Err(e) => {
                log::error!("failed to bind {bind}: {e}");
                std::process::exit(1);
            }
        };
        log::info!("sentinel-flow listening on http://{bind} (REST + SSE at /events)");
        if let Err(e) = axum::serve(listener, app)
            .with_graceful_shutdown(shutdown_signal())
            .await
        {
            log::error!("server error: {e}");
            std::process::exit(1);
        }
    });
    log::info!("sentinel-flow shut down");
}

/// Boot, retrying every 2 s until Postgres is reachable (the app has no fallback).
fn boot_with_retry(database_url: &str) -> std::sync::Arc<AppState> {
    loop {
        match AppState::boot(database_url) {
            Ok(state) => return state,
            Err(e) => {
                log::warn!("boot failed ({e}); retrying in 2s…");
                std::thread::sleep(std::time::Duration::from_secs(2));
            }
        }
    }
}

/// Resolve when the process receives Ctrl-C (or SIGTERM on Unix) for a clean stop.
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
