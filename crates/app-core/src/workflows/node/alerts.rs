use super::{PortValue, PortValues, req_text};
use crate::agent::tools::{ToolContext, ToolUiEvent};
use crate::alerts::{self, AlertLevel};
use serde_json::Value;

pub(super) fn run_get_last_alert(sys: &ToolContext) -> Result<PortValues, String> {
    let mut o = PortValues::new();
    let alerts = sys.alerts.lock().unwrap();
    match alerts.last() {
        Some(a) => {
            o.insert("id".into(), PortValue::Num(a.id as i64));
            o.insert("level".into(), PortValue::Text(a.level.label().to_string()));
            o.insert("message".into(), PortValue::Text(a.message.clone()));
            o.insert("room".into(), PortValue::Text(a.room_name.clone().unwrap_or_default()));
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

pub(super) fn run_create_alert(
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
            let rooms = crate::map::repo::load(&sys.db, sys.tenant_id);
            match rooms.iter().find(|r| r.name.eq_ignore_ascii_case(name.trim())) {
                Some(r) => (Some(r.id), Some(r.name.clone())),
                None => (None, Some(name.to_string())),
            }
        }
        None => (None, None),
    };
    let a = alerts::create(&sys.alerts, Some(&sys.db), sys.tenant_id, level, message, room_id, room_name, source);
    let _ = sys.ui_tx.send(ToolUiEvent::AlertsChanged);
    let mut o = PortValues::new();
    o.insert("id".into(), PortValue::Num(a.id as i64));
    o.insert(
        "result".into(),
        PortValue::Text(format!("Raised alert {} [{}]", a.id, a.level.label())),
    );
    o.insert("ok".into(), PortValue::Bool(true));
    Ok(o)
}

pub(super) fn run_deliver_alert(inputs: &PortValues, sys: &ToolContext) -> Result<PortValues, String> {
    let id = inputs
        .get("id")
        .and_then(PortValue::as_num)
        .ok_or("missing required input 'id'")? as u64;
    let cfg = sys
        .notif_cfg
        .as_ref()
        .ok_or("Notifications are not configured (set the Telegram bot token in Beacons).")?;
    let result = alerts::deliver(&sys.alerts, Some(&sys.db), sys.tenant_id, &sys.notif, cfg, id)?;
    let _ = sys.ui_tx.send(ToolUiEvent::AlertsChanged);
    let mut o = PortValues::new();
    o.insert("result".into(), PortValue::Text(result));
    o.insert("ok".into(), PortValue::Bool(true));
    Ok(o)
}
