//! Postgres NOTIFY event-fabric bridge (outbound only) — card #147, replacing Redis.
//!
//! **Republisher** (out): this pod's local broadcast → `pg_notify('workflow_events', <event-json>)`,
//! so the control-plane SSE relay (which `LISTEN`s on the same Postgres) sees EVERY flow pod's live
//! run-progress events (multi-tenant, multi-pod) with no message broker. This is ephemeral
//! telemetry — the durable *trigger* path (inbound events that fire workflows) already lives in the
//! Postgres `event_queue` (see [`crate::event_consumer`]). Every event JSON carries `tenant`; the
//! control relay filters on it (one channel for all tenants keeps it under the LISTEN name limit).
//!
//! Best-effort + lossy by design. The NOTIFY goes through flow's owned `Db` runtime via
//! `spawn_blocking`, so it never blocks the server runtime.
use std::sync::Arc;

use crate::state::AppState;

/// Spawn the broadcast→Postgres-NOTIFY republisher on the tokio runtime. Returns immediately.
pub fn spawn_republisher(state: Arc<AppState>) {
    log::info!("event fabric: republishing run-progress events via pg_notify('workflow_events')");
    tokio::spawn(async move {
        let mut rx = state.events.subscribe();
        loop {
            let line = match rx.recv().await {
                Ok(l) => l,
                // Lagged: the engine outran this bridge — skip the gap, keep streaming.
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
            };
            // Only forward events that carry a `tenant` (the relay routes on it); skip the rest.
            if serde_json::from_str::<serde_json::Value>(&line)
                .ok()
                .and_then(|v| v.get("tenant").and_then(|t| t.as_str()).map(str::to_string))
                .is_none()
            {
                continue;
            }
            let db = state.db.clone();
            // `Db::notify` blocks on the owned runtime — offload so the server runtime keeps moving.
            tokio::task::spawn_blocking(move || db.notify("workflow_events", &line));
        }
    });
}
