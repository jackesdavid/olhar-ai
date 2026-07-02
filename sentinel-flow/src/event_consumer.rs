//! Durable inbound-event consumer (card #57): claims rows from the shared `event_queue`
//! (`app_core::event_queue`) and fires each into the engine as an `OnEvent` trigger. This
//! replaces the lossy Redis `flow:inject` subscriber — an event now survives a flow restart
//! and is never dropped when the fabric is down.
//!
//! Runs on a std thread; the repo functions bridge sqlx via `Db::block`, so no tokio context is
//! needed here (mirrors [`crate::engine`]'s poller and the notification consumer). Polling is the
//! durable claim mechanism; a Postgres LISTEN/NOTIFY wakeup can front it later as a latency win.
use std::sync::Arc;
use std::time::Duration;

use app_core::event_queue::{self, ClaimedEvent};
use app_core::workflows::EventSource;

use crate::state::AppState;

/// Idle poll interval when the queue is empty.
const IDLE_POLL: Duration = Duration::from_millis(500);

pub fn spawn(state: Arc<AppState>) {
    std::thread::spawn(move || loop {
        match event_queue::claim_next(&state.db) {
            Some(ev) => process(&state, ev),
            None => std::thread::sleep(IDLE_POLL),
        }
    });
    log::info!("inbound event_queue consumer started");
}

fn process(state: &Arc<AppState>, ev: ClaimedEvent) {
    let payload = payload_to_portvalues(Some(&ev.payload));
    // Fire as a System event → fans out to this tenant's OnEvent(event) handler graphs.
    crate::engine::fire_event(
        state,
        ev.tenant_id,
        EventSource::System {
            label: ev.source.clone(),
        },
        ev.event.clone(),
        None,
        payload,
    );
    event_queue::mark_done(&state.db, ev.id);
}

/// Map an inbound JSON payload object to typed `PortValues` (string→Text or Image,
/// integer/float→Num, bool→Bool). Strings that decode as a JPEG (FFD8FF magic) become
/// `Image` — this lets the edge embed the triggering frame inline without a second round-trip.
fn payload_to_portvalues(payload: Option<&serde_json::Value>) -> app_core::workflows::PortValues {
    use app_core::workflows::PortValue;
    use base64::Engine as _;
    let mut pv = app_core::workflows::PortValues::new();
    if let Some(obj) = payload.and_then(|p| p.as_object()) {
        for (k, v) in obj {
            let val = if let Some(arr) = v.as_array() {
                // NumArray ports (e.g. motion `bbox` = [x,y,w,h]) → PortValue::List.
                PortValue::List(arr.iter().map(json_num_to_pv).collect())
            } else if let Some(s) = v.as_str() {
                // Attempt base64 decode; if the result starts with JPEG magic it's an image.
                if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(s.as_bytes()) {
                    if bytes.len() > 3
                        && bytes[0] == 0xFF_u8
                        && bytes[1] == 0xD8
                        && bytes[2] == 0xFF
                    {
                        PortValue::Image(bytes)
                    } else {
                        str_to_pv(s)
                    }
                } else {
                    str_to_pv(s)
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

/// A single JSON number → PortValue::Num (floats rounded). Non-numbers → Text.
fn json_num_to_pv(v: &serde_json::Value) -> app_core::workflows::PortValue {
    use app_core::workflows::PortValue;
    if let Some(n) = v.as_i64() {
        PortValue::Num(n)
    } else if let Some(f) = v.as_f64() {
        PortValue::Num(f.round() as i64)
    } else if let Some(s) = v.as_str() {
        PortValue::Text(s.to_string())
    } else {
        PortValue::Text(v.to_string())
    }
}

/// A scalar string payload value. Defensively decodes a JSON-array or comma-joined
/// number string (e.g. a legacy `bbox` "x,y,w,h") into a NumArray List; otherwise Text.
fn str_to_pv(s: &str) -> app_core::workflows::PortValue {
    use app_core::workflows::PortValue;
    let t = s.trim();
    if t.starts_with('[') {
        if let Ok(serde_json::Value::Array(arr)) = serde_json::from_str::<serde_json::Value>(t) {
            return PortValue::List(arr.iter().map(json_num_to_pv).collect());
        }
    }
    // Comma-joined integers like "12,34,56,78" → List; anything else stays Text.
    let parts: Vec<&str> = t.split(',').map(str::trim).collect();
    if parts.len() >= 2 && parts.iter().all(|p| p.parse::<i64>().is_ok()) {
        return PortValue::List(
            parts
                .iter()
                .map(|p| PortValue::Num(p.parse().unwrap()))
                .collect(),
        );
    }
    PortValue::Text(s.to_string())
}
