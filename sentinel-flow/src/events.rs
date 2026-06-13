//! The SSE event bus. The engine + run executor publish JSON lines here; every
//! connected UI client streams them off the broadcast channel. Mirrors the Tauri
//! shell's `emit(...)` events (`workflow-run`/`-node`/`-log`/`-print`/`-image`)
//! so the existing frontend maps over with minimal change.
//!
//! Per the decomposition plan, **raw image bytes never go on SSE** (it's UTF-8 and
//! one stream — a 300 KB base64 JPEG would head-of-line-block every other event).
//! Image events carry only a reference (`graph`/`node`/`label`); a later phase adds
//! a binary `GET /runs/{id}/frames/{n}` to fetch the JPEG out of band.
use serde_json::{json, Value};
use tokio::sync::broadcast;

use app_core::agent::ToolUiEvent;

/// A cloneable handle to publish events. Lossy by design (SSE is an optimization,
/// never the source of truth — final state is always re-fetchable via REST).
pub type EventBus = broadcast::Sender<String>;

/// Create the bus with a bounded ring buffer (slow clients drop old events, never
/// block the engine).
pub fn channel() -> EventBus {
    broadcast::channel::<String>(512).0
}

/// Publish one named event `{ "event": name, ...fields }` as a JSON line. Send
/// errors (no subscribers) are ignored — the engine never waits on the UI.
pub fn publish(bus: &EventBus, name: &str, mut fields: Value) {
    if let Value::Object(ref mut map) = fields {
        map.insert("event".into(), Value::String(name.to_string()));
    }
    let _ = bus.send(fields.to_string());
}

/// Translate a tool/node UI side-effect into an SSE event, dropping image bytes.
pub fn publish_tool_event(bus: &EventBus, ev: ToolUiEvent) {
    match ev {
        ToolUiEvent::Snapshot { camera, .. } => {
            publish(bus, "snapshot", json!({ "camera": camera }));
        }
        ToolUiEvent::DevicesChanged => publish(bus, "devices-changed", json!({})),
        ToolUiEvent::AlertsChanged => publish(bus, "alerts-changed", json!({})),
        ToolUiEvent::OpenLive { ip } => publish(bus, "open-live", json!({ "ip": ip })),
        ToolUiEvent::Print { message, level, on_screen } => publish(
            bus,
            "workflow-print",
            json!({ "message": message, "level": level, "on_screen": on_screen }),
        ),
        // Reference only — the bytes are fetched out of band (see module note).
        ToolUiEvent::ShowImage { graph, node, label, .. } => {
            publish(bus, "workflow-image", json!({ "graph": graph, "node": node, "label": label }))
        }
    }
}
