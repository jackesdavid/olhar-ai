//! The AI nodes against a self-hosted `llama-server` instead of the Anthropic API.
//!
//! `llama-server` (llama.cpp) serves an OpenAI-shaped `/v1/chat/completions`, so one client
//! covers it and anything else speaking that shape. Selection is by configuration:
//! [`configured`] is true once `LOCAL_LLM_URL` is set, and [`crate::ports::LocalVision`] reads it
//! to pick a provider. There is no runtime failover between providers — a local box that is off
//! fails the node, deliberately, rather than quietly spending money on the hosted API.
//!
//! Every operation here mirrors its [`crate::agent::vision`] counterpart: same system prompt,
//! same token ceiling, same parse. A graph must not behave differently because of which provider
//! answered. `temperature: 0` because these are classifications, not prose — the yes/no and the
//! router arm should not move between runs.

use std::time::Duration;

use serde_json::{json, Value};

use super::imageprep::jpeg_b64;
use super::vision::api_err;

const DESCRIBE_MAX_TOKENS: u32 = 320;
const YES_NO_MAX_TOKENS: u32 = 8;
const ANSWER_MAX_TOKENS: u32 = 640;
const CLASSIFY_MAX_TOKENS: u32 = 8;
const EXTRACT_MAX_TOKENS: u32 = 40;

/// A local box loading a vision model answers far slower than the hosted API, and the first
/// request after start pays for the model load.
const TIMEOUT: Duration = Duration::from_secs(180);

/// Where the local model lives, and how to address it.
pub struct LocalLlm {
    /// Base URL of the server, e.g. `http://100.115.33.124:8080`.
    pub base: String,
    /// Sent as `model`. `llama-server` serves whatever it was started with and ignores this, but
    /// a multi-model front end in front of it will not.
    pub model: String,
    /// Matches `llama-server --api-key`. Empty when the server takes no key.
    pub key: String,
}

impl LocalLlm {
    /// Read `LOCAL_LLM_URL` (required), `LOCAL_LLM_MODEL` and `LOCAL_LLM_KEY` (both optional).
    /// `None` when no URL is set, which is the signal that the hosted provider should answer.
    pub fn from_env() -> Option<Self> {
        let base = std::env::var("LOCAL_LLM_URL").ok()?;
        let base = base.trim().trim_end_matches('/');
        if base.is_empty() {
            return None;
        }
        Some(LocalLlm {
            base: base.to_string(),
            model: std::env::var("LOCAL_LLM_MODEL").unwrap_or_else(|_| "local".into()),
            key: std::env::var("LOCAL_LLM_KEY").unwrap_or_default(),
        })
    }
}

/// Whether the AI nodes should go to the local model. Read by `LocalVision` per call, so moving
/// a deployment between providers is an env change and a restart.
pub fn configured() -> bool {
    LocalLlm::from_env().is_some()
}

/// One round-trip to `/v1/chat/completions`. `image` is a base64 JPEG, sent as a `data:` URL in
/// the `image_url` block the OpenAI shape uses for vision.
fn chat(
    label: &str,
    system: &str,
    text: &str,
    image: Option<String>,
    max_tokens: u32,
) -> Result<String, String> {
    let cfg = LocalLlm::from_env()
        .ok_or_else(|| format!("{label} needs a local model — set LOCAL_LLM_URL."))?;

    let mut content: Vec<Value> = Vec::new();
    if let Some(data) = image {
        content.push(json!({
            "type": "image_url",
            "image_url": { "url": format!("data:image/jpeg;base64,{data}") }
        }));
    }
    content.push(json!({ "type": "text", "text": text }));

    let body = json!({
        "model": cfg.model,
        "max_tokens": max_tokens,
        "temperature": 0,
        "messages": [
            { "role": "system", "content": system },
            { "role": "user", "content": content }
        ]
    });

    let agent = ureq::AgentBuilder::new().timeout(TIMEOUT).build();
    let mut req = agent
        .post(&format!("{}/v1/chat/completions", cfg.base))
        .set("content-type", "application/json");
    if !cfg.key.is_empty() {
        req = req.set("authorization", &format!("Bearer {}", cfg.key));
    }
    let resp = req
        .send_string(&body.to_string())
        .map_err(|e| api_err(label, e))?;

    let parsed: Value = resp
        .into_string()
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .ok_or_else(|| format!("{label}: local model response was not valid JSON."))?;
    parsed
        .pointer("/choices/0/message/content")
        .and_then(Value::as_str)
        .map(|s| s.trim().to_string())
        .ok_or_else(|| format!("{label}: local model response had no content."))
}

/// Local counterpart of [`crate::agent::vision::describe_image`].
pub fn describe_image(raw: &[u8], camera_name: &str) -> Result<String, String> {
    let data = jpeg_b64(raw, "Vision")?;
    let prompt = format!(
        "This is a still frame from a security camera named \"{camera_name}\". Describe what is \
         visible: any people, vehicles, animals, notable objects, activity, and the general \
         setting and lighting. Be concise and factual — a few sentences, no preamble. If the \
         frame is too dark, blurred, or empty to make anything out, say so plainly."
    );
    let text = chat(
        "Vision request",
        "You describe security camera frames factually and concisely.",
        &prompt,
        Some(data),
        DESCRIBE_MAX_TOKENS,
    )?;
    if text.is_empty() {
        Err("The model returned no description.".into())
    } else {
        Ok(text)
    }
}

/// Local counterpart of [`crate::agent::vision::ask_text`]. Backs the `AI Answer` node.
pub fn ask_text(prompt: &str, image: Option<&[u8]>) -> Result<String, String> {
    let data = image.map(|raw| jpeg_b64(raw, "AI Answer")).transpose()?;
    chat(
        "AI Answer request",
        "You are Olhar AI, a calm home + business monitoring assistant. Answer concisely and \
         plainly — no preamble. When shown an image, ground your answer in what's visible.",
        prompt,
        data,
        ANSWER_MAX_TOKENS,
    )
}

/// Local counterpart of [`crate::agent::vision::ask_yes_no`]. Backs the `AI Branch` node.
pub fn ask_yes_no(question: &str, image: Option<&[u8]>) -> Result<bool, String> {
    let data = image.map(|raw| jpeg_b64(raw, "AI Branch")).transpose()?;
    let reply = chat(
        "AI Branch request",
        "You are a camera monitor. Answer ONLY with YES or NO, nothing else.",
        question,
        data,
        YES_NO_MAX_TOKENS,
    )?;
    Ok(reply.to_uppercase().starts_with('Y'))
}

/// Local counterpart of [`crate::agent::vision::classify_intent`]. Backs the `AI Switch` node.
pub fn classify_intent(text: &str, arms: &[String]) -> Result<Option<String>, String> {
    if arms.is_empty() {
        return Ok(None);
    }
    let list = arms
        .iter()
        .enumerate()
        .map(|(i, a)| format!("{}. {a}", i + 1))
        .collect::<Vec<_>>()
        .join("\n");
    let system = format!(
        "You are an intent router. Read the user's message and pick the single option that best \
         matches their intent. Reply with ONLY the option's number (1-{}), or 0 if none is a good \
         match. No words — just the number.",
        arms.len()
    );
    let reply = chat(
        "AI Switch request",
        &system,
        &format!("Options:\n{list}\n\nMessage: {text}"),
        None,
        CLASSIFY_MAX_TOKENS,
    )?;
    let n: usize = reply
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>()
        .parse()
        .unwrap_or(0);
    if (1..=arms.len()).contains(&n) {
        Ok(Some(arms[n - 1].clone()))
    } else {
        Ok(None)
    }
}

/// Local counterpart of [`crate::agent::vision::extract_field`]. Backs the `AI Extract` node.
pub fn extract_field(text: &str, field: &str) -> Result<String, String> {
    let system = format!(
        "You extract a single value from a user's message. Extract {field}. Reply with ONLY that \
         value, exactly as the user wrote it (no quotes, no extra words). If it is not present in \
         the message, reply with an empty line."
    );
    chat(
        "AI Extract request",
        &system,
        text,
        None,
        EXTRACT_MAX_TOKENS,
    )
}
