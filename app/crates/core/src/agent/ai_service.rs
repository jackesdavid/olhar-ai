//! Client for the portable AI sidecar (the Python service under `services/ai`,
//! deployed to RunPod Serverless or run locally with `--rp_serve_api`).
//!
//! Calls are **op-dispatched**: we POST `{ "input": { "op": <name>, ...args } }`
//! and the RunPod runsync envelope returns `{ "output": ... }`. Configured via the
//! `.env` keys `AI_SERVICE_URL` and `AI_SERVICE_KEY`, mirroring how `vision.rs`
//! reads `ANTHROPIC_API_KEY` — when unset, callers get a clear "not configured"
//! error instead of a panic.
use std::time::Duration;

use base64::{engine::general_purpose::STANDARD as B64, Engine};
use serde_json::{json, Value};

/// Where to reach the sidecar + the optional bearer key.
pub struct AiServiceConfig {
    pub url: String,
    pub key: Option<String>,
}

impl AiServiceConfig {
    /// Read `AI_SERVICE_URL` (required) and `AI_SERVICE_KEY` (optional, blank for
    /// local dev) from the environment. `None` when no URL is configured.
    pub fn from_env() -> Option<Self> {
        let url = std::env::var("AI_SERVICE_URL").ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())?;
        let key = std::env::var("AI_SERVICE_KEY").ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
        Some(AiServiceConfig { url, key })
    }
}

/// Base64-encode image bytes for the sidecar's `image_b64` field.
pub fn image_b64(bytes: &[u8]) -> String {
    B64.encode(bytes)
}

/// Call an op on the AI sidecar and return its `output` object.
///
/// `input` should be a JSON object of the op's args (e.g. `{ "image_b64": ... }`);
/// the `op` field is merged in here. A 30s+ timeout absorbs the occasional cold
/// start (runsync completes synchronously for these short vision jobs).
pub fn call(op: &str, mut input: Value) -> Result<Value, String> {
    let cfg = AiServiceConfig::from_env().ok_or(
        "AI service not configured — set AI_SERVICE_URL (and AI_SERVICE_KEY) in .env.",
    )?;

    match input.as_object_mut() {
        Some(map) => {
            map.insert("op".into(), json!(op));
        }
        None => input = json!({ "op": op }),
    }

    let agent = ureq::AgentBuilder::new().timeout(Duration::from_secs(120)).build();
    let mut req = agent.post(&cfg.url).set("Content-Type", "application/json");
    if let Some(k) = &cfg.key {
        req = req.set("Authorization", &format!("Bearer {k}"));
    }
    let body = req
        .send_string(&json!({ "input": input }).to_string())
        .map_err(|e| format!("AI service request failed: {e}"))?
        .into_string()
        .map_err(|e| format!("AI service: couldn't read response: {e}"))?;
    let v: Value =
        serde_json::from_str(&body).map_err(|e| format!("AI service returned invalid JSON: {e}"))?;

    // RunPod runsync envelope: { status, output }. A FAILED/CANCELLED/TIMED_OUT
    // status (or a missing output) is an error; local --rp_serve_api returns the
    // same shape. IN_PROGRESS would mean the job outlived runsync's wait — not
    // expected for these short jobs, so treat a missing output as an error.
    if let Some(status) = v.get("status").and_then(Value::as_str) {
        if status != "COMPLETED" {
            let detail = v.get("error").and_then(Value::as_str).unwrap_or(status);
            return Err(format!("AI service job {status}: {detail}"));
        }
    }
    let out = v.get("output").cloned().unwrap_or(v);
    if let Some(err) = out.get("error").and_then(Value::as_str) {
        return Err(format!("AI op '{op}': {err}"));
    }
    Ok(out)
}
