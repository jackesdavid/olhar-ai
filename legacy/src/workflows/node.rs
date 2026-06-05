//! Workflows node layer — the isolation boundary (spec 013, Phase 2).
//!
//! A node declares typed ports; the engine resolves its inputs and hands it ONLY its
//! own `PortValues`, taking back ONLY its declared outputs. A node never sees the graph
//! or another node.
//!
//! Deviations from `architecture.md` (surfaced for review):
//!  * `PortValue` is a typed enum (not `serde_json::Value`) so an `Image` port can carry
//!    raw JPEG bytes (`Vec<u8>`) — JSON can't hold image bytes cleanly.
//!  * Action nodes adapt the underlying domain functions (`alerts::actions`,
//!    `notification::dispatch`, `device::actions`, `agent::vision`) directly rather than
//!    the JSON `ToolRegistry` — typed in/out, no fragile string-result parsing. The system
//!    handle is the existing `ToolContext` (store / ui_tx / notif / alerts).
#![allow(dead_code)]

use super::domain::{NodeKind, Port, PortType};
use crate::agent::tools::{ToolContext, ToolUiEvent};
use crate::alerts::{actions as alert_actions, AlertLevel};
use crate::device::actions as dev_actions;
use serde_json::Value;
use std::collections::HashMap;

/// A typed value flowing along an edge / sitting on a port.
#[derive(Debug, Clone)]
pub enum PortValue {
    Text(String),
    Num(i64),
    Bool(bool),
    Image(Vec<u8>),
}

impl PortValue {
    pub fn as_text(&self) -> Option<&str> {
        match self {
            PortValue::Text(s) => Some(s),
            _ => None,
        }
    }
    pub fn as_num(&self) -> Option<i64> {
        match self {
            PortValue::Num(n) => Some(*n),
            _ => None,
        }
    }
    pub fn as_image(&self) -> Option<&[u8]> {
        match self {
            PortValue::Image(b) => Some(b),
            _ => None,
        }
    }
}

/// One node's resolved port values (inputs the engine assembles, or outputs a node returns).
pub type PortValues = HashMap<String, PortValue>;

/// Convert a node's config literal into a typed value for an UNCONNECTED input port.
/// Empty text / missing keys / `Image` ports yield `None` (treated as "not provided").
pub fn literal_for(config: &Value, port: Port) -> Option<PortValue> {
    let v = config.get(port.name)?;
    match port.ty {
        PortType::Text => {
            let s = v.as_str()?;
            if s.trim().is_empty() {
                None
            } else {
                Some(PortValue::Text(s.to_string()))
            }
        }
        PortType::Num => {
            if let Some(n) = v.as_i64() {
                Some(PortValue::Num(n))
            } else {
                v.as_str()
                    .and_then(|s| s.trim().parse::<i64>().ok())
                    .map(PortValue::Num)
            }
        }
        PortType::Bool => v.as_bool().map(PortValue::Bool),
        PortType::Image => None,
    }
}

/// Execute one node against its resolved inputs. Returns its declared outputs.
pub fn run_node(
    kind: NodeKind,
    config: &Value,
    inputs: &PortValues,
    sys: &ToolContext,
) -> Result<PortValues, String> {
    match kind {
        NodeKind::GetLastAlert => run_get_last_alert(sys),
        NodeKind::CreateAlert => run_create_alert(config, inputs, sys),
        NodeKind::DeliverAlert => run_deliver_alert(inputs, sys),
        NodeKind::CameraSnapshot => run_camera_snapshot(inputs, sys),
        NodeKind::DescribeImage => run_describe_image(config, inputs),
        NodeKind::NotifyBeacon => run_notify_beacon(inputs, sys),
        NodeKind::Format => run_format(config, inputs),
    }
}

fn req_text<'a>(inputs: &'a PortValues, name: &str) -> Result<&'a str, String> {
    inputs
        .get(name)
        .and_then(PortValue::as_text)
        .ok_or_else(|| format!("missing required input '{name}'"))
}

fn run_get_last_alert(sys: &ToolContext) -> Result<PortValues, String> {
    let mut o = PortValues::new();
    let alerts = sys.alerts.lock().unwrap();
    match alerts.last() {
        Some(a) => {
            o.insert("id".into(), PortValue::Num(a.id as i64));
            o.insert("level".into(), PortValue::Text(a.level.label().to_string()));
            o.insert("message".into(), PortValue::Text(a.message.clone()));
            o.insert(
                "room".into(),
                PortValue::Text(a.room_name.clone().unwrap_or_default()),
            );
            o.insert("source".into(), PortValue::Text(a.source.clone()));
            o.insert("created_at".into(), PortValue::Text(a.created_at.clone()));
            o.insert("ok".into(), PortValue::Bool(true));
        }
        None => {
            o.insert("ok".into(), PortValue::Bool(false));
        }
    }
    Ok(o)
}

fn run_create_alert(
    config: &Value,
    inputs: &PortValues,
    sys: &ToolContext,
) -> Result<PortValues, String> {
    let message = req_text(inputs, "message")?.to_string();
    let level = config
        .get("level")
        .and_then(Value::as_str)
        .and_then(AlertLevel::parse)
        .unwrap_or_default();
    let source = config
        .get("source")
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .unwrap_or("workflow")
        .to_string();
    let (room_id, room_name) = match inputs
        .get("room")
        .and_then(PortValue::as_text)
        .filter(|s| !s.trim().is_empty())
    {
        Some(name) => {
            let rooms = crate::map::domain::load_map();
            match rooms.iter().find(|r| r.name.eq_ignore_ascii_case(name.trim())) {
                Some(r) => (Some(r.id), Some(r.name.clone())),
                None => (None, Some(name.to_string())),
            }
        }
        None => (None, None),
    };
    let a = alert_actions::create(&sys.alerts, level, message, room_id, room_name, source);
    let _ = sys.ui_tx.send(ToolUiEvent::AlertsPersisted);
    let mut o = PortValues::new();
    o.insert("id".into(), PortValue::Num(a.id as i64));
    o.insert(
        "result".into(),
        PortValue::Text(format!("Raised alert {} [{}]", a.id, a.level.label())),
    );
    o.insert("ok".into(), PortValue::Bool(true));
    Ok(o)
}

fn run_deliver_alert(inputs: &PortValues, sys: &ToolContext) -> Result<PortValues, String> {
    let id = inputs
        .get("id")
        .and_then(PortValue::as_num)
        .ok_or("missing required input 'id'")? as u64;
    let result = alert_actions::deliver(&sys.alerts, &sys.notif, id)?;
    let _ = sys.ui_tx.send(ToolUiEvent::AlertsPersisted);
    let mut o = PortValues::new();
    o.insert("result".into(), PortValue::Text(result));
    o.insert("ok".into(), PortValue::Bool(true));
    Ok(o)
}

fn run_camera_snapshot(inputs: &PortValues, sys: &ToolContext) -> Result<PortValues, String> {
    let camera = req_text(inputs, "camera")?;
    let device = {
        let devices = sys.store.lock().unwrap();
        let idx = dev_actions::resolve_device(&devices, camera).map_err(|e| e.to_string())?;
        devices[idx].clone()
    };
    let bytes = dev_actions::capture_snapshot_bytes(&device)?;
    // Mirror the chat tool: also surface the snapshot in the UI modal.
    if let Ok(img) = dev_actions::decode_luma(&bytes) {
        let _ = sys.ui_tx.send(ToolUiEvent::Image(img));
    }
    let mut o = PortValues::new();
    o.insert("image".into(), PortValue::Image(bytes));
    o.insert(
        "result".into(),
        PortValue::Text(format!("Snapshot of '{}' captured", device.name)),
    );
    o.insert("ok".into(), PortValue::Bool(true));
    Ok(o)
}

fn run_describe_image(config: &Value, inputs: &PortValues) -> Result<PortValues, String> {
    let bytes = inputs
        .get("image")
        .and_then(PortValue::as_image)
        .ok_or("missing required input 'image'")?;
    let label = config
        .get("label")
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .unwrap_or("camera");
    let description = crate::agent::vision::describe_image(bytes, label)?;
    let mut o = PortValues::new();
    o.insert("description".into(), PortValue::Text(description));
    o.insert("ok".into(), PortValue::Bool(true));
    Ok(o)
}

fn run_notify_beacon(inputs: &PortValues, sys: &ToolContext) -> Result<PortValues, String> {
    let message = req_text(inputs, "message")?;
    let target = inputs
        .get("target")
        .and_then(PortValue::as_text)
        .filter(|s| !s.trim().is_empty());
    let image = inputs
        .get("image")
        .and_then(PortValue::as_image)
        .map(|b| b.to_vec());
    let cfg = crate::notification::NotificationConfig::from_env()
        .ok_or("Notifications are not configured (set TELEGRAM_BOT_TOKEN).")?;
    let result = crate::notification::dispatch(&sys.notif, &cfg, target, message, image)?;
    let mut o = PortValues::new();
    o.insert("result".into(), PortValue::Text(result));
    o.insert("ok".into(), PortValue::Bool(true));
    Ok(o)
}

fn run_format(config: &Value, inputs: &PortValues) -> Result<PortValues, String> {
    let template = config.get("template").and_then(Value::as_str).unwrap_or("");
    let get = |k: &str| {
        inputs
            .get(k)
            .and_then(PortValue::as_text)
            .unwrap_or("")
            .to_string()
    };
    let text = template
        .replace("{p1}", &get("p1"))
        .replace("{p2}", &get("p2"))
        .replace("{p3}", &get("p3"));
    let mut o = PortValues::new();
    o.insert("text".into(), PortValue::Text(text));
    Ok(o)
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::domain::NodeKind;
    use serde_json::json;

    #[test]
    fn literal_for_coerces_and_skips_empty() {
        let cfg = json!({ "camera": "front", "blank": "  ", "n": "7" });
        let text = Port { name: "camera", ty: PortType::Text, required: true };
        let blank = Port { name: "blank", ty: PortType::Text, required: false };
        let num = Port { name: "n", ty: PortType::Num, required: true };
        assert_eq!(literal_for(&cfg, text).unwrap().as_text(), Some("front"));
        assert!(literal_for(&cfg, blank).is_none(), "blank text is 'not provided'");
        assert_eq!(literal_for(&cfg, num).unwrap().as_num(), Some(7));
    }

    #[test]
    fn format_substitutes_present_inputs_only() {
        let cfg = json!({ "template": "{p1} — {p2}" });
        let mut inputs = PortValues::new();
        inputs.insert("p1".into(), PortValue::Text("door".into()));
        // p2 absent -> replaced with empty
        let out = run_node(NodeKind::Format, &cfg, &inputs, &test_ctx()).unwrap();
        assert_eq!(out["text"].as_text(), Some("door — "));
    }

    #[test]
    fn create_alert_requires_message_before_any_io() {
        // Empty default message + no wire -> error BEFORE alert_actions::create runs,
        // so nothing is written to disk.
        let ctx = test_ctx();
        let cfg = NodeKind::CreateAlert.default_config();
        let err = run_node(NodeKind::CreateAlert, &cfg, &PortValues::new(), &ctx).unwrap_err();
        assert!(err.contains("missing required input 'message'"), "got: {err}");
        assert!(ctx.alerts.lock().unwrap().is_empty());
    }

    fn test_ctx() -> ToolContext {
        let (tx, _rx) = std::sync::mpsc::channel();
        ToolContext {
            store: std::sync::Arc::new(std::sync::Mutex::new(vec![])),
            ui_tx: tx,
            notif: std::sync::Arc::new(std::sync::Mutex::new(vec![])),
            alerts: std::sync::Arc::new(std::sync::Mutex::new(vec![])),
        }
    }
}
