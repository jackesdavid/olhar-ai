//! Redis event-fabric bridge.
//!
//! Republishes this pod's local broadcast events to Redis channel `events:<tenant>` so the
//! control-plane SSE relay sees EVERY flow pod's events (multi-tenant, multi-pod) instead of
//! one pod's in-process broadcast. Best-effort + auto-reconnecting; a no-op when `REDIS_URL`
//! is unset (local dev without Redis still streams off the in-process broadcast via the old
//! `/events` route).
use std::sync::Arc;
use std::time::Duration;

use crate::state::AppState;

/// Spawn the broadcast→Redis republisher on the tokio runtime. Returns immediately.
pub fn spawn_republisher(state: Arc<AppState>) {
    let Some(url) = std::env::var("REDIS_URL").ok().filter(|s| !s.trim().is_empty()) else {
        log::info!("REDIS_URL unset — Redis event fabric disabled (local broadcast only)");
        return;
    };
    log::info!("Redis event fabric: republishing events to {url}");
    tokio::spawn(async move {
        loop {
            if let Err(e) = run(&state, &url).await {
                log::warn!("redis republisher error: {e}; reconnecting in 2s");
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    });
}

async fn run(state: &Arc<AppState>, url: &str) -> Result<(), String> {
    let client = redis::Client::open(url).map_err(|e| e.to_string())?;
    let mut conn = client.get_connection_manager().await.map_err(|e| e.to_string())?;
    let mut rx = state.events.subscribe();
    loop {
        let line = match rx.recv().await {
            Ok(l) => l,
            // Lagged: the engine outran this bridge — skip the gap, keep streaming.
            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
            Err(tokio::sync::broadcast::error::RecvError::Closed) => return Ok(()),
        };
        // Each event JSON carries `tenant`; route it to that tenant's channel.
        let Some(tenant) = serde_json::from_str::<serde_json::Value>(&line)
            .ok()
            .and_then(|v| v.get("tenant").and_then(|t| t.as_str()).map(str::to_string))
        else {
            continue;
        };
        let _: i64 = redis::cmd("PUBLISH")
            .arg(format!("events:{tenant}"))
            .arg(&line)
            .query_async(&mut conn)
            .await
            .map_err(|e| e.to_string())?;
    }
}
