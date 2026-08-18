//! `sentinel-notification` — the notification service (card #57).
//!
//! Owns recipient config + the actual sending. Flow's notify nodes only ENQUEUE into the
//! durable `notification_queue`; this service claims rows, resolves the tenant's recipient
//! config, sends (Telegram), and marks each terminal with retry/backoff. Config REST + the
//! read-only observability endpoints + the inbound pairing loop land in later phases.
//!
//! Run:
//!   DATABASE_URL=postgres://sentinel:sentinel@localhost/sentinel \
//!   NOTIFY_BIND=0.0.0.0:8791 cargo run -p sentinel-notification
//!
//! Optional env: `TELEGRAM_BOT_TOKEN` (global send-token fallback when not set in Postgres).
mod consumer;
mod http;
mod pairing;
mod state;

use std::sync::Arc;

use state::AppState;

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://sentinel:sentinel@localhost/sentinel".to_string());
    let bind = std::env::var("NOTIFY_BIND").unwrap_or_else(|_| "0.0.0.0:8791".to_string());

    // Boot SYNCHRONOUSLY, before any tokio runtime: `Db::connect` drives its own runtime with
    // `block_on` (would panic inside async). Retry so we survive Postgres coming up after us.
    let state = boot_with_retry(&database_url);

    // The queue consumer runs on its own std thread (blocking); `Db::block` bridges sqlx.
    consumer::spawn(state.clone());
    consumer::spawn_media_purge(state.clone());
    consumer::spawn_push_receipts(state.clone());
    // This service now owns Telegram pairing (moved from sentinel-flow).
    pairing::spawn(state.clone());

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
        log::info!("sentinel-notification listening on http://{bind}");
        if let Err(e) = axum::serve(listener, app)
            .with_graceful_shutdown(shutdown_signal())
            .await
        {
            log::error!("server error: {e}");
            std::process::exit(1);
        }
    });
    log::info!("sentinel-notification shut down");
}

fn boot_with_retry(database_url: &str) -> Arc<AppState> {
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
