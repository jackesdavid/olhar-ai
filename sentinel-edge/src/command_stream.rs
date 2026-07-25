//! Cloud→edge command stream client (card #144).
//!
//! The edge holds ONE outbound SSE GET to control (`/config/edge/stream`). Commands arrive as
//! `event: command` frames; each is run against the edge's OWN local HTTP API (`127.0.0.1:<port>`)
//! — reusing the exact recording flow the app drives, so there is a single code path — and the
//! result is POSTed back to `/config/edge/command-result`.
//!
//! This is how the cloud commands a box it cannot reach inbound: the box is outbound-only (a phone
//! with no tailnet, and eventually every edge once the tailnet is retired). Blocking + `ureq`,
//! matching the edge's thread model; reconnects with backoff.

use std::io::{BufRead, BufReader};
use std::time::Duration;

const RECONNECT_MIN_SECS: u64 = 2;
const RECONNECT_MAX_SECS: u64 = 30;
/// The stream sends a keep-alive comment every ~15s, so a read gap longer than this means the
/// connection is dead — bounded so a silently-dropped socket is noticed and reconnected.
const READ_TIMEOUT_SECS: u64 = 60;

/// Hold the command stream open forever, reconnecting with backoff. Spawned from `run()` once the
/// edge is enrolled (it needs a box token to authenticate). `local_api` is this edge's own HTTP
/// base (e.g. `http://127.0.0.1:8788`) — commands are dispatched to it.
pub fn run(control_url: String, local_api: String) {
    let mut backoff = RECONNECT_MIN_SECS;
    loop {
        match pump(&control_url, &local_api) {
            Ok(()) => backoff = RECONNECT_MIN_SECS, // clean EOF → reconnect promptly
            Err(e) => log::warn!("command stream: {e}; reconnecting in {backoff}s"),
        }
        std::thread::sleep(Duration::from_secs(backoff));
        backoff = (backoff * 2).min(RECONNECT_MAX_SECS);
    }
}

fn token() -> Option<String> {
    std::env::var("BOX_TOKEN").ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

/// Connect and read SSE frames until the stream ends or errors. Returns Ok on a clean EOF.
fn pump(control_url: &str, local_api: &str) -> Result<(), String> {
    let Some(tok) = token() else { return Err("no BOX_TOKEN — not enrolled".into()) };
    let url = format!("{control_url}/config/edge/stream");
    // A dedicated agent with a bounded READ timeout (not an overall one) so the never-ending stream
    // isn't killed mid-flight, but a dead socket is still detected between keep-alives.
    let agent = ureq::AgentBuilder::new()
        .timeout_read(Duration::from_secs(READ_TIMEOUT_SECS))
        .build();
    let resp = agent
        .get(&url)
        .set("Authorization", &format!("Bearer {tok}"))
        .set("Accept", "text/event-stream")
        .set("X-Edge-Protocol", "1")
        .call()
        .map_err(|e| format!("connect: {e}"))?;
    log::info!("command stream: connected to {url}");

    let reader = BufReader::new(resp.into_reader());
    let mut event = String::new();
    let mut data = String::new();
    for line in reader.lines() {
        let line = line.map_err(|e| format!("read: {e}"))?;
        if line.is_empty() {
            // Blank line terminates an SSE event — dispatch what we accumulated.
            if event == "command" && !data.is_empty() {
                handle(control_url, local_api, &tok, &data);
            }
            event.clear();
            data.clear();
            continue;
        }
        if let Some(v) = line.strip_prefix("event:") {
            event = v.trim().to_string();
        } else if let Some(v) = line.strip_prefix("data:") {
            if !data.is_empty() {
                data.push('\n');
            }
            data.push_str(v.strip_prefix(' ').unwrap_or(v));
        }
        // `:` comments (keep-alives), `id:`, `retry:` are ignored.
    }
    Ok(())
}

/// Parse one command frame `{v,id,cmd,args}`, run it locally, and report the result.
fn handle(control_url: &str, local_api: &str, tok: &str, data: &str) {
    let v: serde_json::Value = match serde_json::from_str(data) {
        Ok(v) => v,
        Err(e) => {
            log::warn!("command stream: bad frame ({e})");
            return;
        }
    };
    let id = v.get("id").and_then(|x| x.as_str()).unwrap_or_default().to_string();
    let cmd = v.get("cmd").and_then(|x| x.as_str()).unwrap_or_default();
    let args = v.get("args").cloned().unwrap_or_else(|| serde_json::json!({}));
    log::info!("command stream: <- {cmd} (id={id})");

    let (ok, result, error) = dispatch(local_api, cmd, &args);

    // Report the outcome so the waiting cloud caller wakes.
    let agent = ureq::AgentBuilder::new().timeout(Duration::from_secs(30)).build();
    let body = serde_json::json!({ "id": id, "ok": ok, "result": result, "error": error });
    if let Err(e) = agent
        .post(&format!("{control_url}/config/edge/command-result"))
        .set("Authorization", &format!("Bearer {tok}"))
        .send_json(body)
    {
        log::warn!("command stream: result POST failed for {id}: {e}");
    }
}

/// Dispatch a command to the edge's own local HTTP API — the same endpoints the app drives, so the
/// cloud path and the local path are one and the same.
fn dispatch(local_api: &str, cmd: &str, args: &serde_json::Value) -> (bool, Option<serde_json::Value>, Option<String>) {
    let agent = ureq::AgentBuilder::new().timeout(Duration::from_secs(120)).build();
    let post = |path: &str| -> Result<serde_json::Value, String> {
        match agent.post(&format!("{local_api}{path}")).send_json(args.clone()) {
            Ok(resp) => resp.into_json::<serde_json::Value>().map_err(|e| format!("decode: {e}")),
            Err(ureq::Error::Status(code, resp)) => {
                Err(resp.into_string().unwrap_or_else(|_| format!("HTTP {code}")))
            }
            Err(e) => Err(format!("{e}")),
        }
    };
    let out = match cmd {
        "start_recording" => post("/recording/start"),
        "stop_recording" => post("/recording/stop"),
        "snapshot" => {
            // GET /snapshot/<ip> returns JPEG bytes; not wired through this JSON path yet.
            Err("snapshot over the command stream is not implemented yet".into())
        }
        other => Err(format!("unknown command: {other}")),
    };
    match out {
        Ok(r) => (true, Some(r), None),
        Err(e) => (false, None, Some(e)),
    }
}
