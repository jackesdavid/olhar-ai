//! The alert workflow nodes, now backed by the agnostic event stream (card #66): CreateAlert
//! records an `alert` event, GetLastAlert reads the newest one, DeliverAlert delivers it to
//! beacons. The nodes are unchanged for users — only the store beneath them moved from the retired
//! `alerts` table to `events`.
use super::{req_text, PortValue, PortValues};
use crate::agent::tools::ToolContext;
use crate::events::{self, AlertLevel, NewEvent};
use serde_json::Value;

pub(super) fn run_get_last_alert(sys: &ToolContext) -> Result<PortValues, String> {
    let mut o = PortValues::new();
    match events::latest_of_kind_sync(&sys.db, sys.tenant_id, "alert") {
        Some(e) => {
            o.insert("id".into(), PortValue::Num(e.id));
            o.insert("level".into(), PortValue::Text(e.severity.to_uppercase()));
            o.insert(
                "message".into(),
                PortValue::Text(e.message.unwrap_or_default()),
            );
            o.insert(
                "room".into(),
                PortValue::Text(e.room_name.unwrap_or_default()),
            );
            o.insert("source".into(), PortValue::Text(e.source));
            o.insert("created_at".into(), PortValue::Text(e.occurred_at));
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
            match rooms
                .iter()
                .find(|r| r.name.eq_ignore_ascii_case(name.trim()))
            {
                Some(r) => (Some(r.id as i64), Some(r.name.clone())),
                None => (None, Some(name.to_string())),
            }
        }
        None => (None, None),
    };

    let mut ev = NewEvent::new(sys.tenant_id, "alert", source);
    ev.severity = level.severity().to_string();
    ev.message = Some(message);
    ev.room_id = room_id;
    ev.room_name = room_name;
    let id =
        events::insert_event_returning_sync(&sys.db, ev).ok_or("failed to record alert event")?;

    let mut o = PortValues::new();
    o.insert("id".into(), PortValue::Num(id));
    o.insert(
        "result".into(),
        PortValue::Text(format!("Raised alert {id} [{}]", level.label())),
    );
    o.insert("ok".into(), PortValue::Bool(true));
    Ok(o)
}

pub(super) fn run_deliver_alert(
    inputs: &PortValues,
    sys: &ToolContext,
) -> Result<PortValues, String> {
    let id = inputs
        .get("id")
        .and_then(PortValue::as_num)
        .ok_or("missing required input 'id'")?;
    let cfg = sys
        .notif_cfg
        .as_ref()
        .ok_or("Notifications are not configured (set the Telegram bot token in Beacons).")?;
    let result = events::deliver_event_to_beacons(&sys.db, sys.tenant_id, id, &sys.notif, cfg)?;
    let mut o = PortValues::new();
    o.insert("result".into(), PortValue::Text(result));
    o.insert("ok".into(), PortValue::Bool(true));
    Ok(o)
}
