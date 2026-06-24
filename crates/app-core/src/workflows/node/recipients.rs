use super::{PortValue, PortValues, req_text};
use crate::agent::tools::ToolContext;
use serde_json::Value;

pub(super) fn run_get_recipient(config: &Value) -> Result<PortValues, String> {
    let label = config.get("recipient").and_then(Value::as_str).unwrap_or("").trim().to_string();
    let ok = !label.is_empty();
    let mut o = PortValues::new();
    o.insert("recipient".into(), PortValue::Recipient(label));
    o.insert("ok".into(), PortValue::Bool(ok));
    Ok(o)
}

pub(super) fn run_notify_recipient(inputs: &PortValues, sys: &ToolContext) -> Result<PortValues, String> {
    let message = req_text(inputs, "message")?;
    let target = inputs
        .get("recipient")
        .and_then(PortValue::as_recipient)
        .filter(|s| !s.trim().is_empty());
    let image = inputs.get("image").and_then(PortValue::as_image).map(|b| b.to_vec());
    let cfg = sys
        .notif_cfg
        .as_ref()
        .ok_or("Notifications are not configured (set the Telegram bot token in Beacons).")?;
    let result = sys.notifier.dispatch(&sys.notif, cfg, target, message, image)?;
    let mut o = PortValues::new();
    o.insert("result".into(), PortValue::Text(result));
    o.insert("ok".into(), PortValue::Bool(true));
    Ok(o)
}
