//! Telegram bot-brain nodes (card #15): reply to the chat that triggered the run — the counterpart
//! to `NotifyRecipient` (which addresses configured beacons). Both reach Telegram only through the
//! `Notifier` port (`sys.notifier`), so in prod they enqueue onto the durable queue and the
//! notification service sends, while local dev sends in-process.
use super::{PortValue, PortValues};
use crate::agent::tools::ToolContext;
use serde_json::Value;

/// `chat_id` is the pivot of a bot reply — read it as a Num, erroring with a hint to wire it from
/// the `On event("telegram_message")` entry.
fn req_chat_id(inputs: &PortValues) -> Result<i64, String> {
    inputs
        .get("chat_id")
        .and_then(PortValue::as_num)
        .ok_or_else(|| {
            "chat_id is required — wire it from On event(\"telegram_message\").chat_id".to_string()
        })
}

/// The tenant's Telegram config, or a friendly error when notifications aren't set up.
fn req_cfg(sys: &ToolContext) -> Result<&crate::notification::NotificationConfig, String> {
    sys.notif_cfg
        .as_ref()
        .ok_or("Notifications are not configured (set the Telegram bot token in Recipients).")
        .map_err(str::to_string)
}

pub(super) fn run_reply_to_sender(
    config: &Value,
    inputs: &PortValues,
    sys: &ToolContext,
) -> Result<PortValues, String> {
    let message = inputs
        .get("message")
        .and_then(PortValue::as_text)
        .or_else(|| config.get("message").and_then(Value::as_str))
        .unwrap_or("")
        .to_string();
    let chat_id = req_chat_id(inputs)?;
    let image = inputs
        .get("image")
        .and_then(PortValue::as_image)
        .map(<[u8]>::to_vec);
    if message.trim().is_empty() && image.is_none() {
        return Err("Reply to sender: nothing to send — wire a message or an image".into());
    }
    let cfg = req_cfg(sys)?;
    let result = sys.notifier.reply(cfg, chat_id, &message, image)?;
    Ok(reply_outputs(result))
}

pub(super) fn run_reply_with_recording(
    config: &Value,
    inputs: &PortValues,
    sys: &ToolContext,
) -> Result<PortValues, String> {
    let clip_id = inputs
        .get("clip_id")
        .and_then(PortValue::as_text)
        .filter(|s| !s.trim().is_empty())
        .ok_or("Reply with recording: clip_id is required (wire it from Start recording)")?;
    let chat_id = req_chat_id(inputs)?;
    let caption = inputs
        .get("caption")
        .and_then(PortValue::as_text)
        .or_else(|| config.get("caption").and_then(Value::as_str))
        .unwrap_or("")
        .to_string();
    let cfg = req_cfg(sys)?;
    // Fetch the clip bytes from this tenant's edge, then relay as a video.
    let mp4 = super::recording::fetch_clip(sys, clip_id)?;
    let result = sys.notifier.reply_video(cfg, chat_id, &caption, mp4)?;
    Ok(reply_outputs(result))
}

/// Shared outputs for the reply nodes: `queued` (true once the send is accepted/enqueued) + the
/// `result` confirmation string ("queued" on the durable path, "sent" in-process).
fn reply_outputs(result: String) -> PortValues {
    let mut o = PortValues::new();
    o.insert("queued".into(), PortValue::Bool(true));
    o.insert("result".into(), PortValue::Text(result));
    o
}
