//! Telegram Bot API calls (blocking `ureq`). `sendMessage` for text, `sendPhoto`
//! for text + image (manual multipart), and `getUpdates` long-poll for the
//! pairing flow. Copied verbatim from the ratatui `src/notification/client.rs`.
//! The bot token sits in the URL path, so error strings here deliberately never
//! include the URL.
use std::io::Read;
use std::time::Duration;

use serde_json::{json, Value};

use super::domain::NotificationConfig;

const API_BASE: &str = "https://api.telegram.org";
const BOUNDARY: &str = "----bishopTelegramBoundary7MA4YWxkTrZu0gW";

fn method_url(cfg: &NotificationConfig, method: &str) -> String {
    format!("{API_BASE}/bot{}/{method}", cfg.bot_token)
}

/// Send a plain text message to a chat.
pub fn send_message(cfg: &NotificationConfig, chat_id: i64, text: &str) -> Result<(), String> {
    let body = json!({ "chat_id": chat_id, "text": text });
    let resp = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(15))
        .build()
        .post(&method_url(cfg, "sendMessage"))
        .set("Content-Type", "application/json")
        .send_string(&body.to_string())
        .map_err(|e| format!("sendMessage failed: {e}"))?;
    check_ok(resp)
}

/// Send a photo (raw JPEG bytes) with a caption to a chat.
pub fn send_photo(
    cfg: &NotificationConfig,
    chat_id: i64,
    caption: &str,
    jpeg: &[u8],
) -> Result<(), String> {
    if jpeg.is_empty() {
        return Err("no image bytes to send".into());
    }
    let body = photo_multipart(chat_id, caption, jpeg);
    let content_type = format!("multipart/form-data; boundary={BOUNDARY}");
    let resp = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(30))
        .build()
        .post(&method_url(cfg, "sendPhoto"))
        .set("Content-Type", &content_type)
        .send_bytes(&body)
        .map_err(|e| format!("sendPhoto failed: {e}"))?;
    check_ok(resp)
}

/// Send a video (raw mp4 bytes) with a caption to a chat. Used for `Reply with recording`
/// (card #15) — a recording clip fetched from the edge, relayed to the sender.
pub fn send_video(
    cfg: &NotificationConfig,
    chat_id: i64,
    caption: &str,
    mp4: &[u8],
) -> Result<(), String> {
    if mp4.is_empty() {
        return Err("no video bytes to send".into());
    }
    let body = media_multipart(chat_id, caption, "video", "clip.mp4", "video/mp4", mp4);
    let content_type = format!("multipart/form-data; boundary={BOUNDARY}");
    let resp = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(120)) // video upload to Telegram can be slow
        .build()
        .post(&method_url(cfg, "sendVideo"))
        .set("Content-Type", &content_type)
        .send_bytes(&body)
        .map_err(|e| format!("sendVideo failed: {e}"))?;
    check_ok(resp)
}

fn photo_multipart(chat_id: i64, caption: &str, jpeg: &[u8]) -> Vec<u8> {
    media_multipart(
        chat_id,
        caption,
        "photo",
        "snapshot.jpg",
        "image/jpeg",
        jpeg,
    )
}

/// Build a Telegram multipart body with `chat_id` + `caption` text parts and one binary media
/// part (`field` = "photo"/"video", with its filename + MIME). Shared by send_photo/send_video.
fn media_multipart(
    chat_id: i64,
    caption: &str,
    field: &str,
    filename: &str,
    mime: &str,
    bytes: &[u8],
) -> Vec<u8> {
    let mut body = Vec::with_capacity(bytes.len() + 512);
    let mut push = |s: &str| body.extend_from_slice(s.as_bytes());

    push(&format!("--{BOUNDARY}\r\n"));
    push("Content-Disposition: form-data; name=\"chat_id\"\r\n\r\n");
    push(&chat_id.to_string());
    push("\r\n");

    push(&format!("--{BOUNDARY}\r\n"));
    push("Content-Disposition: form-data; name=\"caption\"\r\n\r\n");
    push(caption);
    push("\r\n");

    push(&format!("--{BOUNDARY}\r\n"));
    push(&format!(
        "Content-Disposition: form-data; name=\"{field}\"; filename=\"{filename}\"\r\n"
    ));
    push(&format!("Content-Type: {mime}\r\n\r\n"));
    body.extend_from_slice(bytes);
    body.extend_from_slice(b"\r\n");

    body.extend_from_slice(format!("--{BOUNDARY}--\r\n").as_bytes());
    body
}

/// Fetch the bot's own profile and return its `username`. Used to build the
/// `t.me/<username>?start=<code>` deep-link for one-tap pairing on mobile.
pub fn get_me(cfg: &NotificationConfig) -> Result<String, String> {
    let resp = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(10))
        .build()
        .get(&method_url(cfg, "getMe"))
        .call()
        .map_err(|e| format!("getMe failed: {e}"))?;
    let body = resp
        .into_string()
        .map_err(|e| format!("read failed: {e}"))?;
    let v: Value = serde_json::from_str(&body).map_err(|e| format!("bad JSON: {e}"))?;
    if v.get("ok").and_then(Value::as_bool) != Some(true) {
        return Err("getMe returned ok=false".into());
    }
    v.get("result")
        .and_then(|r| r.get("username"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| "bot has no username".into())
}

/// A minimal projection of a Telegram update we care about for pairing.
pub struct Update {
    pub update_id: i64,
    pub chat_id: i64,
    pub text: String,
    /// Sender label: first name, else username, else "operator".
    pub sender_label: String,
}

/// Long-poll for updates newer than `offset`. `timeout_s` is the server-side
/// long-poll hold; the HTTP read timeout is set comfortably above it so `ureq`
/// doesn't abort the held-open connection.
pub fn get_updates(
    cfg: &NotificationConfig,
    offset: i64,
    timeout_s: u32,
) -> Result<Vec<Update>, String> {
    let resp = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(timeout_s as u64 + 10))
        .build()
        .post(&method_url(cfg, "getUpdates"))
        .set("Content-Type", "application/json")
        .send_string(
            &json!({ "offset": offset, "timeout": timeout_s, "allowed_updates": ["message"] })
                .to_string(),
        )
        .map_err(|e| format!("getUpdates failed: {e}"))?;

    let mut raw = String::new();
    resp.into_reader()
        .read_to_string(&mut raw)
        .map_err(|e| format!("read failed: {e}"))?;
    let value: Value = serde_json::from_str(&raw).map_err(|e| format!("bad response: {e}"))?;
    if value.get("ok").and_then(Value::as_bool) != Some(true) {
        return Err("getUpdates returned ok=false".into());
    }
    Ok(parse_updates(&value))
}

/// Pull the fields we care about out of a getUpdates response (pure; testable).
fn parse_updates(value: &Value) -> Vec<Update> {
    value
        .get("result")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|u| {
                    let update_id = u.get("update_id")?.as_i64()?;
                    let msg = u.get("message")?;
                    let chat_id = msg.get("chat")?.get("id")?.as_i64()?;
                    let text = msg
                        .get("text")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    let from = msg.get("from");
                    let sender_label = from
                        .and_then(|f| f.get("first_name"))
                        .and_then(Value::as_str)
                        .or_else(|| from.and_then(|f| f.get("username")).and_then(Value::as_str))
                        .unwrap_or("operator")
                        .to_string();
                    Some(Update {
                        update_id,
                        chat_id,
                        text,
                        sender_label,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Read a Telegram response body and verify `ok: true`, surfacing the API's
/// `description` on failure.
fn check_ok(resp: ureq::Response) -> Result<(), String> {
    let body = resp
        .into_string()
        .map_err(|e| format!("read failed: {e}"))?;
    let v: Value = serde_json::from_str(&body).map_err(|e| format!("bad response: {e}"))?;
    if v.get("ok").and_then(Value::as_bool) == Some(true) {
        Ok(())
    } else {
        Err(v
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or("Telegram API error")
            .to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_updates_extracts_fields_and_name_fallback() {
        let v = json!({
            "ok": true,
            "result": [
                { "update_id": 10, "message": { "chat": { "id": 42 }, "text": "/pair X",
                  "from": { "first_name": "Dave" } } },
                { "update_id": 11, "message": { "chat": { "id": 7 }, "text": "hi",
                  "from": { "username": "ripley" } } },
                { "update_id": 12 } // no message — skipped
            ]
        });
        let ups = parse_updates(&v);
        assert_eq!(ups.len(), 2);
        assert_eq!(ups[0].chat_id, 42);
        assert_eq!(ups[0].sender_label, "Dave");
        assert_eq!(ups[1].sender_label, "ripley");
    }

    #[test]
    fn photo_multipart_has_required_parts() {
        let body = photo_multipart(99, "hello", &[0xFF, 0xD8, 0xFF]);
        let head = String::from_utf8_lossy(&body);
        assert!(head.contains("name=\"chat_id\""));
        assert!(head.contains("name=\"caption\""));
        assert!(head.contains("filename=\"snapshot.jpg\""));
        assert!(head.contains("Content-Type: image/jpeg"));
        assert!(head.trim_end().ends_with(&format!("--{BOUNDARY}--")));
    }
}
