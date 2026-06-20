//! Redis event-fabric bridge.
//!
//! Two directions:
//! - **Republisher** (out): this pod's local broadcast → Redis channel `events:<tenant>`, so
//!   the control-plane SSE relay sees EVERY flow pod's events (multi-tenant, multi-pod).
//! - **Injector** (in): Redis channel `flow:inject` → `engine::fire_event` as a `System`
//!   event, so externally-originated events (edge motion via control) trigger `OnEvent`
//!   handler graphs. Messages are `{ tenant, event, payload:{field:value,…} }`.
//!
//! Both are best-effort + auto-reconnecting; no-ops when `REDIS_URL` is unset.
use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt;

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

/// Spawn the Redis `flow:inject` subscriber: turns externally-injected events (edge motion
/// relayed by control) into `fire_event(System, …)` so they trigger `OnEvent` handlers.
pub fn spawn_injector(state: Arc<AppState>) {
    let Some(url) = std::env::var("REDIS_URL").ok().filter(|s| !s.trim().is_empty()) else {
        return; // republisher already logged the disabled-fabric notice
    };
    tokio::spawn(async move {
        loop {
            if let Err(e) = run_injector(&state, &url).await {
                log::warn!("redis injector error: {e}; reconnecting in 2s");
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    });
}

async fn run_injector(state: &Arc<AppState>, url: &str) -> Result<(), String> {
    let client = redis::Client::open(url).map_err(|e| e.to_string())?;
    let mut pubsub = client.get_async_pubsub().await.map_err(|e| e.to_string())?;
    pubsub.subscribe("flow:inject").await.map_err(|e| e.to_string())?;
    log::info!("Redis event fabric: injector subscribed to flow:inject");
    let mut on_msg = pubsub.on_message();
    while let Some(msg) = on_msg.next().await {
        let Ok(raw) = msg.get_payload::<String>() else { continue };
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw) else { continue };
        let tenant = v
            .get("tenant")
            .and_then(|t| t.as_str())
            .and_then(|s| s.parse::<uuid::Uuid>().ok());
        let event = v.get("event").and_then(|e| e.as_str()).map(str::to_string);
        let (Some(tenant), Some(event)) = (tenant, event) else { continue };
        let payload = payload_to_portvalues(v.get("payload"));
        // Fire as a System event → fans out to this tenant's OnEvent(event) handler graphs.
        crate::engine::fire_event(
            state,
            tenant,
            app_core::workflows::EventSource::System { label: "edge".to_string() },
            event,
            None,
            payload,
        );
    }
    Ok(())
}

/// Map an injected JSON payload object to typed `PortValues` (string→Text or Image,
/// integer/float→Num, bool→Bool). Strings that decode as a JPEG (FFD8FF magic) become
/// `Image` — this lets the edge embed the triggering frame inline without a second round-trip.
fn payload_to_portvalues(payload: Option<&serde_json::Value>) -> app_core::workflows::PortValues {
    use app_core::workflows::PortValue;
    use base64::Engine as _;
    let mut pv = app_core::workflows::PortValues::new();
    if let Some(obj) = payload.and_then(|p| p.as_object()) {
        for (k, v) in obj {
            let val = if let Some(s) = v.as_str() {
                // Attempt base64 decode; if the result starts with JPEG magic it's an image.
                if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(s.as_bytes()) {
                    if bytes.len() > 3 && bytes[0] == 0xFF_u8 && bytes[1] == 0xD8 && bytes[2] == 0xFF {
                        PortValue::Image(bytes)
                    } else {
                        PortValue::Text(s.to_string())
                    }
                } else {
                    PortValue::Text(s.to_string())
                }
            } else if let Some(n) = v.as_i64() {
                PortValue::Num(n)
            } else if let Some(f) = v.as_f64() {
                PortValue::Num(f.round() as i64)
            } else if let Some(b) = v.as_bool() {
                PortValue::Bool(b)
            } else {
                continue;
            };
            pv.insert(k.clone(), val);
        }
    }
    pv
}
