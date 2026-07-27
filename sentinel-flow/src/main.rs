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
mod event_consumer;
mod events;
mod http;
mod schedule_consumer;
mod state;
mod window_consumer;

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
    // Structured logging → Datadog (card #146): every log ships with service:flow + span context.
    app_core::datadog::init_tracing("flow");

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
    // Durable inbound triggers: claim from the Postgres event_queue and fire OnEvent handlers
    // (card #57 — replaces the lossy Redis flow:inject injector). Runs on its own std thread.
    event_consumer::spawn(state.clone());
    // Durable timing windows: deliver a DelayTrigger's trailing `stop` when its quiet window
    // elapses (card #86). Resumes any window that was armed when the pod went down.
    window_consumer::spawn(state.clone());
    schedule_consumer::spawn(state.clone());
    // Telegram pairing moved to sentinel-notification (card #57) — flow no longer polls.

    // Multi-pod graph-store sync: control emits `pg_notify('graphs_reload')` on every workflow
    // mutation; EVERY flow pod LISTENs and reloads. Fixes the round-robin gap where control's HTTP
    // poke reached only one replica behind the `sentinel-flow` service DNS, so the other pod ran a
    // stale store and silently dropped events it "had no graph" for. Runs on the Db's own runtime.
    {
        let reload_state = state.clone();
        state.db.listen("graphs_reload", move |_payload| {
            reload_state.reload_graphs();
            let (n, enabled) = {
                let s = reload_state.store.lock().unwrap();
                (s.len(), s.iter().filter(|(_, g)| g.enabled).count())
            };
            log::info!(
                "graphs reloaded via NOTIFY — {n} graph(s), {enabled} enabled (pod {})",
                reload_state.pod_name
            );
        });
    }

    // Now build the tokio runtime for the HTTP control plane.
    let rt = tokio::runtime::Runtime::new().expect("build tokio runtime");
    rt.block_on(async move {
        // Bridge this pod's live run-progress events onto the Postgres NOTIFY fabric (card #147,
        // multi-pod SSE relay). Inbound triggers arrive via the durable event_queue consumer above.
        bus::spawn_republisher(state.clone());
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
