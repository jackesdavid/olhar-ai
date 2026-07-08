//! Client for the portable AI sidecar (the Python service under `ai/sidecar`,
//! deployed to RunPod Serverless or run locally with `--rp_serve_api`).
//!
//! Calls are **op-dispatched**: we POST `{ "input": { "op": <name>, ...args } }`
//! and the RunPod runsync envelope returns `{ "output": ... }`. Configured via the
//! `.env` keys `AI_SERVICE_URL` and `AI_SERVICE_KEY`, mirroring how `vision.rs`
//! reads `ANTHROPIC_API_KEY` — when unset, callers get a clear "not configured"
//! error instead of a panic.
use std::time::{Duration, Instant};

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
    ///
    /// The URL is normalised to a `/runsync` endpoint: a bare host or RunPod base
    /// (`http://localhost:8000`, `https://api.runpod.ai/v2/<id>`) gets `/runsync`
    /// appended, while a URL already ending in `/runsync` or `/run` is left as-is.
    pub fn from_env() -> Option<Self> {
        let raw = std::env::var("AI_SERVICE_URL").ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())?;
        let base = raw.trim_end_matches('/');
        let url = if base.ends_with("/runsync") || base.ends_with("/run") {
            base.to_string()
        } else {
            format!("{base}/runsync")
        };
        let key = std::env::var("AI_SERVICE_KEY").ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
        Some(AiServiceConfig { url, key })
    }
}

/// Base64-encode image bytes for the sidecar's `image_b64` field.
pub fn image_b64(bytes: &[u8]) -> String {
    B64.encode(bytes)
}

/// An AI-sidecar call's `output` plus the RunPod-billed time (milliseconds).
///
/// `delay_ms` is queue/cold-start (`delayTime`), `exec_ms` is worker execution
/// (`executionTime`) — together they are what RunPod bills, and what billing attributes
/// per tenant. Local (`--rp_serve_api`) calls report no timing, so both are `0`.
pub struct CallOutcome {
    pub output: Value,
    pub delay_ms: u32,
    pub exec_ms: u32,
}

/// Call an op on the AI sidecar and return its `output` plus the RunPod-billed time.
///
/// Times the full round trip — request + any cold-start `/status` polling — and logs
/// the start (with the target URL, so local-GPU vs RunPod is visible) and, on finish,
/// both the wall-clock and the RunPod-billed (`delay + exec`) milliseconds. The billed
/// time is what [`crate::billing`] attributes per tenant.
pub fn call(op: &str, input: Value) -> Result<CallOutcome, String> {
    let started = Instant::now();
    let result = call_inner(op, input);
    let ms = started.elapsed().as_millis();
    match &result {
        Ok(o) => log::info!(
            "ai_service: op '{op}' finished in {ms} ms (billed: delay {} + exec {} ms)",
            o.delay_ms, o.exec_ms
        ),
        Err(e) => log::warn!("ai_service: op '{op}' failed after {ms} ms: {e}"),
    }
    result
}

/// Inner call: merge `op` into `input`, POST it, and resolve the RunPod envelope
/// (polling `/status` while a cold-start job is `IN_QUEUE`/`IN_PROGRESS`).
///
/// `input` should be a JSON object of the op's args (e.g. `{ "image_b64": ... }`);
/// the `op` field is merged in here.
fn call_inner(op: &str, mut input: Value) -> Result<CallOutcome, String> {
    let cfg = AiServiceConfig::from_env().ok_or(
        "AI service not configured — set AI_SERVICE_URL (and AI_SERVICE_KEY) in .env.",
    )?;
    log::info!("ai_service: op '{op}' → {}", cfg.url);

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

    let (out, delay_ms, exec_ms) = resolve_output(&cfg, &agent, v)?;
    if let Some(err) = out.get("error").and_then(Value::as_str) {
        return Err(format!("AI op '{op}': {err}"));
    }
    Ok(CallOutcome { output: out, delay_ms, exec_ms })
}

/// Resolve the RunPod envelope to `(output, delay_ms, exec_ms)`, polling `/status/<id>`
/// while the job is `IN_QUEUE`/`IN_PROGRESS` (cold start). Local `--rp_serve_api` returns
/// `COMPLETED` synchronously (no timing fields → `0`), so the loop runs zero times there.
fn resolve_output(cfg: &AiServiceConfig, agent: &ureq::Agent, mut v: Value) -> Result<(Value, u32, u32), String> {
    // `<base>/runsync` → `<base>/status/<id>` for polling.
    let base = cfg.url.trim_end_matches("/runsync").trim_end_matches("/run");
    let mut tries = 0;
    loop {
        match v.get("status").and_then(Value::as_str).unwrap_or("COMPLETED") {
            "COMPLETED" => break,
            "IN_QUEUE" | "IN_PROGRESS" => {
                let id = v
                    .get("id")
                    .and_then(Value::as_str)
                    .ok_or("AI service job is pending but returned no job id to poll")?;
                // Cap the cold-start poll (~2 min) so a stuck job can't keep an orphaned
                // worker thread alive far past its node's deadline. The always-warm VPS
                // sidecar returns COMPLETED synchronously, so this loop runs zero times there.
                if tries >= 40 {
                    return Err("AI service timed out — job still pending (cold start?)".into());
                }
                tries += 1;
                std::thread::sleep(Duration::from_secs(3));
                let mut req = agent.get(&format!("{base}/status/{id}"));
                if let Some(k) = &cfg.key {
                    req = req.set("Authorization", &format!("Bearer {k}"));
                }
                let body = req
                    .call()
                    .map_err(|e| format!("AI service status poll failed: {e}"))?
                    .into_string()
                    .map_err(|e| format!("AI service: couldn't read status: {e}"))?;
                v = serde_json::from_str(&body)
                    .map_err(|e| format!("AI service returned invalid status JSON: {e}"))?;
            }
            other => {
                let detail = v.get("error").and_then(Value::as_str).unwrap_or(other);
                return Err(format!("AI service job {other}: {detail}"));
            }
        }
    }
    // Billed time lives at the envelope top level (siblings of `output`). Absent on a
    // local sidecar → 0. Read before moving `v` into the `output` fallback.
    let delay_ms = v.get("delayTime").and_then(Value::as_u64).unwrap_or(0) as u32;
    let exec_ms = v.get("executionTime").and_then(Value::as_u64).unwrap_or(0) as u32;
    let output = v.get("output").cloned().unwrap_or(v);
    Ok((output, delay_ms, exec_ms))
}
