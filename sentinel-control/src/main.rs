//! `sentinel-control` — the control plane.
//!
//! Owns the central source of truth and serves it to edge appliances so they can be
//! **config-light** (no Postgres on the box). Today: the camera list
//! (`GET /config/cameras`). Later: tenants, `tenant_boxes`, the enrollment broker,
//! heartbeats, and the web app's backend API.
//!
//! Run:
//!   DATABASE_URL=postgres://sentinel:sentinel@localhost/sentinel \
//!   CONTROL_BIND=0.0.0.0:8790 cargo run -p sentinel-control   # or: task control
use std::sync::Arc;

use app_core::db::Db;

mod http;

/// Shared state — just the DB handle for now.
pub struct AppState {
    pub db: Db,
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://sentinel:sentinel@localhost/sentinel".to_string());
    let bind = std::env::var("CONTROL_BIND").unwrap_or_else(|_| "0.0.0.0:8790".to_string());

    // Boot synchronously, before any tokio runtime (Db::connect drives its own
    // runtime with block_on). Retry until Postgres is reachable.
    let state = boot_with_retry(&database_url);

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
        log::info!("sentinel-control listening on http://{bind} (GET /config/cameras)");
        if let Err(e) = axum::serve(listener, app).with_graceful_shutdown(shutdown_signal()).await {
            log::error!("server error: {e}");
            std::process::exit(1);
        }
    });
    log::info!("sentinel-control shut down");
}

fn boot_with_retry(database_url: &str) -> Arc<AppState> {
    loop {
        match Db::connect(database_url) {
            Ok(db) => {
                log::info!("Postgres connected; migrations applied");
                return Arc::new(AppState { db });
            }
            Err(e) => {
                log::warn!("connect Postgres failed ({e}); retrying in 2s…");
                std::thread::sleep(std::time::Duration::from_secs(2));
            }
        }
    }
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
