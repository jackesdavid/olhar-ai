//! One-shot image description via Claude vision. The chat `describe_camera` tool
//! feeds a camera snapshot through here to get a short, factual account of what's
//! visible. Blocking on purpose — callers run it on a background task. Copied
//! verbatim from the ratatui `src/agent/vision.rs`.
use std::time::Duration;

use serde_json::{json, Value};

use super::domain::{text_of, AgentConfig};
use super::imageprep::jpeg_b64;

const ENDPOINT: &str = "https://api.anthropic.com/v1/messages";
const ANTHROPIC_VERSION: &str = "2023-06-01";
const MAX_TOKENS: u32 = 320; // a few sentences of description
const YES_NO_MAX_TOKENS: u32 = 8; // "YES" or "NO" only
const ANSWER_MAX_TOKENS: u32 = 640; // a short free-text answer (Telegram reply length)

/// Turn a ureq failure into a message that carries the API's own explanation.
///
/// `ureq::Error`'s `Display` renders a non-2xx as bare "status code 400" and drops the body —
/// where Anthropic puts the only text saying WHICH field was rejected. Status also separates the
/// causes that get confused: 400 is a malformed request or an exhausted credit balance, 401 is a
/// bad key, 404 an unknown model, 429 a rate limit.
pub(crate) fn api_err(label: &str, err: ureq::Error) -> String {
    match err {
        ureq::Error::Status(code, resp) => {
            let body = resp.into_string().unwrap_or_default();
            let detail = serde_json::from_str::<Value>(&body)
                .ok()
                .and_then(|v| {
                    v.pointer("/error/message")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                })
                .unwrap_or_else(|| body.trim().chars().take(400).collect());
            format!("{label} failed: HTTP {code} — {detail}")
        }
        ureq::Error::Transport(t) => format!("{label} failed: {t}"),
    }
}

/// Describe the contents of a camera snapshot (any image format the `image`
/// crate can decode). Returns the description text, or an error string suitable
/// for showing the operator (e.g. when no API key is configured).
pub fn describe_image(raw: &[u8], camera_name: &str) -> Result<String, String> {
    let cfg = AgentConfig::from_env()
        .ok_or("Image description needs an ANTHROPIC_API_KEY — none configured.")?;

    let data = jpeg_b64(raw, "Vision")?;

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

    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(60))
        .build();
    let resp = agent
        .post(ENDPOINT)
        .set("x-api-key", &cfg.api_key)
        .set("anthropic-version", ANTHROPIC_VERSION)
        .set("content-type", "application/json")
        .send_string(&body.to_string())
        .map_err(|e| api_err("Vision request", e))?;
    let parsed: Value = resp
        .into_string()
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .ok_or("Vision response was not valid JSON.")?;
    let content = parsed
        .get("content")
        .ok_or("Vision response had no content.")?;

    let text = text_of(content).trim().to_string();
    if text.is_empty() {
        Err("The model returned no description.".into())
    } else {
        Ok(text)
    }
}

/// Single-turn free-text answer to `prompt`, with an optional image for vision. One round-trip
/// (no tool-use loop) so it's fast enough for a Telegram reply. Backs the `AI Answer` node.
pub fn ask_text(prompt: &str, image: Option<&[u8]>) -> Result<String, String> {
    let cfg =
        AgentConfig::from_env().ok_or("AI Answer needs an ANTHROPIC_API_KEY — none configured.")?;

    let mut content: Vec<Value> = Vec::new();
    if let Some(raw) = image {
        let data = jpeg_b64(raw, "AI Answer")?;
        content.push(json!({
            "type": "image",
            "source": { "type": "base64", "media_type": "image/jpeg", "data": data }
        }));
    }
    content.push(json!({ "type": "text", "text": prompt }));

    let body = json!({
        "model": cfg.model,
        "max_tokens": ANSWER_MAX_TOKENS,
        "system": "You are Olhar AI, a calm home + business monitoring assistant. Answer concisely \
                   and plainly — no preamble. When shown an image, ground your answer in what's visible.",
        "messages": [{ "role": "user", "content": content }]
    });

    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(60))
        .build();
    let resp = agent
        .post(ENDPOINT)
        .set("x-api-key", &cfg.api_key)
        .set("anthropic-version", ANTHROPIC_VERSION)
        .set("content-type", "application/json")
        .send_string(&body.to_string())
        .map_err(|e| api_err("AI Answer request", e))?;
    let parsed: Value = resp
        .into_string()
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .ok_or("AI Answer response was not valid JSON.")?;
    let text = text_of(
        parsed
            .get("content")
            .ok_or("AI Answer response had no content.")?,
    )
    .trim()
    .to_string();
    if text.is_empty() {
        Err("The model returned an empty answer.".into())
    } else {
        Ok(text)
    }
}

/// Route `text` to one of `arms` (intent labels) for the AI Switch node. Returns the matched arm
/// (verbatim from `arms`), or `None` for no confident match → the caller fires the `default`/unknown
/// branch. The model replies with only the option NUMBER (validated back to an index), so a
/// hallucinated label can never mis-route. One constrained round-trip (`max_tokens: 8`).
pub fn classify_intent(text: &str, arms: &[String]) -> Result<Option<String>, String> {
    if arms.is_empty() {
        return Ok(None);
    }
    let cfg =
        AgentConfig::from_env().ok_or("AI Switch needs an ANTHROPIC_API_KEY — none configured.")?;
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
    let body = json!({
        "model": cfg.model,
        "max_tokens": 8,
        "system": system,
        "messages": [{ "role": "user", "content": format!("Options:\n{list}\n\nMessage: {text}") }]
    });

    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(30))
        .build();
    let resp = agent
        .post(ENDPOINT)
        .set("x-api-key", &cfg.api_key)
        .set("anthropic-version", ANTHROPIC_VERSION)
        .set("content-type", "application/json")
        .send_string(&body.to_string())
        .map_err(|e| api_err("AI Switch request", e))?;
    let parsed: Value = resp
        .into_string()
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .ok_or("AI Switch response was not valid JSON.")?;
    let reply = text_of(
        parsed
            .get("content")
            .ok_or("AI Switch response had no content.")?,
    );
    // Parse the leading integer the model returned; map 1-based index → arm, else None.
    let n: usize = reply
        .trim()
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

/// Extract one field from a message for the AI Extract node (slot filling). `field` describes what
/// to pull out ("the camera name", "the recipient's name"); returns the extracted value verbatim,
/// or an empty string when it's not present. One constrained call (`max_tokens: 40`).
pub fn extract_field(text: &str, field: &str) -> Result<String, String> {
    let cfg = AgentConfig::from_env()
        .ok_or("AI Extract needs an ANTHROPIC_API_KEY — none configured.")?;
    let system = format!(
        "You extract a single value from a user's message. Extract {field}. Reply with ONLY that \
         value, exactly as the user wrote it (no quotes, no extra words). If it is not present in \
         the message, reply with an empty line."
    );
    let body = json!({
        "model": cfg.model,
        "max_tokens": 40,
        "system": system,
        "messages": [{ "role": "user", "content": text }]
    });
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(30))
        .build();
    let resp = agent
        .post(ENDPOINT)
        .set("x-api-key", &cfg.api_key)
        .set("anthropic-version", ANTHROPIC_VERSION)
        .set("content-type", "application/json")
        .send_string(&body.to_string())
        .map_err(|e| api_err("AI Extract request", e))?;
    let parsed: Value = resp
        .into_string()
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .ok_or("AI Extract response was not valid JSON.")?;
    Ok(text_of(
        parsed
            .get("content")
            .ok_or("AI Extract response had no content.")?,
    )
    .trim()
    .to_string())
}

/// Ask a yes/no question about an optional image frame. Returns `true` for YES, `false`
/// for NO. The model is forced to one-word answers via the system prompt and `max_tokens: 8`.
/// When `image` is `None` the question is asked as plain text (useful for time/logic checks).
pub fn ask_yes_no(question: &str, image: Option<&[u8]>) -> Result<bool, String> {
    let cfg =
        AgentConfig::from_env().ok_or("AI If needs an ANTHROPIC_API_KEY — none configured.")?;

    let mut content: Vec<Value> = Vec::new();
    if let Some(raw) = image {
        let data = jpeg_b64(raw, "AI If")?;
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

    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(60))
        .build();
    let resp = agent
        .post(ENDPOINT)
        .set("x-api-key", &cfg.api_key)
        .set("anthropic-version", ANTHROPIC_VERSION)
        .set("content-type", "application/json")
        .send_string(&body.to_string())
        .map_err(|e| api_err("AI Branch request", e))?;
    let parsed: Value = resp
        .into_string()
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .ok_or("AI If response was not valid JSON.")?;
    let text = text_of(
        parsed
            .get("content")
            .ok_or("AI If response had no content.")?,
    )
    .trim()
    .to_uppercase();
    Ok(text.starts_with('Y'))
}
