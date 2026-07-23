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

    // Phone recipients (card #139). A registered device is its OWN Recipient — that's the
    // granularity the operator asked for (one send addresses one phone; recipient *groups* come
    // later). So resolve the label against `push_devices` FIRST: a push-only tenant has no Telegram
    // config, and the `notif_cfg` check below would otherwise fail the node before push is tried.
    // Queued (not sent inline) so push inherits the same durable retry/backoff as Telegram.
    if let Some(label) = target {
        let tokens = crate::notification::push::enabled_tokens(&sys.db, sys.tenant_id, Some(label));
        if !tokens.is_empty() {
            crate::work_queue::enqueue_push_notification_blocking(
                &sys.db,
                sys.tenant_id,
                Some(label),
                "Sentinel",
                message,
            );
            let mut o = PortValues::new();
            o.insert(
                "result".into(),
                PortValue::Text(format!("queued push to {} device(s) ({label})", tokens.len())),
            );
            o.insert("ok".into(), PortValue::Bool(true));
            return Ok(o);
        }
    }

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
