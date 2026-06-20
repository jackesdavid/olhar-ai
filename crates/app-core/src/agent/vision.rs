//! One-shot image description via Claude vision. The chat `describe_camera` tool
//! feeds a camera snapshot through here to get a short, factual account of what's
//! visible. Blocking on purpose — callers run it on a background task. Copied
//! verbatim from the ratatui `src/agent/vision.rs`.
use std::io::Cursor;
use std::time::Duration;

use base64::{engine::general_purpose::STANDARD as B64, Engine};
use image::ImageFormat;
use serde_json::{json, Value};

use super::domain::{text_of, AgentConfig};

const ENDPOINT: &str = "https://api.anthropic.com/v1/messages";
const ANTHROPIC_VERSION: &str = "2023-06-01";
const MAX_TOKENS: u32 = 320; // a few sentences of description
const YES_NO_MAX_TOKENS: u32 = 8; // "YES" or "NO" only
/// Cameras can be 1080p+; Claude downsizes anything past ~1568px anyway, so cap
/// the long edge here to keep the base64 payload small without losing detail.
const MAX_EDGE: u32 = 1024;

/// Describe the contents of a camera snapshot (any image format the `image`
/// crate can decode). Returns the description text, or an error string suitable
/// for showing the operator (e.g. when no API key is configured).
pub fn describe_image(raw: &[u8], camera_name: &str) -> Result<String, String> {
    let cfg = AgentConfig::from_env()
        .ok_or("Image description needs an ANTHROPIC_API_KEY — none configured.")?;

    // Decode and re-encode to a bounded JPEG so the media type is predictable
    // (the snapshot may arrive as JPEG over HTTP or a frame from the RTSP
    // fallback) and the payload stays modest.
    let img = image::load_from_memory(raw).map_err(|e| format!("Could not decode snapshot: {e}"))?;
    let img = if img.width().max(img.height()) > MAX_EDGE {
        img.resize(MAX_EDGE, MAX_EDGE, image::imageops::FilterType::Triangle)
    } else {
        img
    };
    let mut jpeg = Vec::new();
    img.write_to(&mut Cursor::new(&mut jpeg), ImageFormat::Jpeg)
        .map_err(|e| format!("Could not encode snapshot: {e}"))?;
    let data = B64.encode(&jpeg);

    let prompt = format!(
        "This is a still frame from a security camera named \"{camera_name}\". Describe what is \
         visible: any people, vehicles, animals, notable objects, activity, and the general \
         setting and lighting. Be concise and factual — a few sentences, no preamble. If the \
         frame is too dark, blurred, or empty to make anything out, say so plainly."
    );
    let body = json!({
        "model": cfg.model,
        "max_tokens": MAX_TOKENS,
        "messages": [{
            "role": "user",
            "content": [
                { "type": "image", "source": { "type": "base64", "media_type": "image/jpeg", "data": data } },
                { "type": "text", "text": prompt }
            ]
        }]
    });

    let agent = ureq::AgentBuilder::new().timeout(Duration::from_secs(60)).build();
    let resp = agent
        .post(ENDPOINT)
        .set("x-api-key", &cfg.api_key)
        .set("anthropic-version", ANTHROPIC_VERSION)
        .set("content-type", "application/json")
        .send_string(&body.to_string())
        .map_err(|e| format!("Vision request failed: {e}"))?;
    let parsed: Value = resp
        .into_string()
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .ok_or("Vision response was not valid JSON.")?;
    let content = parsed.get("content").ok_or("Vision response had no content.")?;

    let text = text_of(content).trim().to_string();
    if text.is_empty() {
        Err("The model returned no description.".into())
    } else {
        Ok(text)
    }
}

/// Ask a yes/no question about an optional image frame. Returns `true` for YES, `false`
/// for NO. The model is forced to one-word answers via the system prompt and `max_tokens: 8`.
/// When `image` is `None` the question is asked as plain text (useful for time/logic checks).
pub fn ask_yes_no(question: &str, image: Option<&[u8]>) -> Result<bool, String> {
    let cfg = AgentConfig::from_env()
        .ok_or("AI If needs an ANTHROPIC_API_KEY — none configured.")?;

    let mut content: Vec<Value> = Vec::new();
    if let Some(raw) = image {
        let img = image::load_from_memory(raw)
            .map_err(|e| format!("AI If: could not decode image: {e}"))?;
        let img = if img.width().max(img.height()) > MAX_EDGE {
            img.resize(MAX_EDGE, MAX_EDGE, image::imageops::FilterType::Triangle)
        } else {
            img
        };
        let mut jpeg = Vec::new();
        img.write_to(&mut Cursor::new(&mut jpeg), ImageFormat::Jpeg)
            .map_err(|e| format!("AI If: could not encode image: {e}"))?;
        let data = B64.encode(&jpeg);
        content.push(json!({
            "type": "image",
            "source": { "type": "base64", "media_type": "image/jpeg", "data": data }
        }));
    }
    content.push(json!({ "type": "text", "text": question }));

    let body = json!({
        "model": cfg.model,
        "max_tokens": YES_NO_MAX_TOKENS,
        "system": "You are a camera monitor. Answer ONLY with YES or NO, nothing else.",
        "messages": [{ "role": "user", "content": content }]
    });

    let agent = ureq::AgentBuilder::new().timeout(Duration::from_secs(60)).build();
    let resp = agent
        .post(ENDPOINT)
        .set("x-api-key", &cfg.api_key)
        .set("anthropic-version", ANTHROPIC_VERSION)
        .set("content-type", "application/json")
        .send_string(&body.to_string())
        .map_err(|e| format!("AI If request failed: {e}"))?;
    let parsed: Value = resp
        .into_string()
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .ok_or("AI If response was not valid JSON.")?;
    let text = text_of(parsed.get("content").ok_or("AI If response had no content.")?)
        .trim()
        .to_uppercase();
    Ok(text.starts_with('Y'))
}
