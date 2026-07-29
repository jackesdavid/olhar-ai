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
use std::sync::Arc;
use std::time::Duration;

use crate::state::AppState;

/// The edge's shared state, stashed so the `start_live` handler can start a camera pipeline. Set
/// once at boot; only the gst-linked live path reads it.
#[cfg(feature = "gst-linked")]
static STATE: std::sync::OnceLock<Arc<AppState>> = std::sync::OnceLock::new();

const RECONNECT_MIN_SECS: u64 = 2;
const RECONNECT_MAX_SECS: u64 = 30;
/// The stream sends a keep-alive comment every ~15s, so a read gap longer than this means the
/// connection is dead — bounded so a silently-dropped socket is noticed and reconnected.
const READ_TIMEOUT_SECS: u64 = 60;

/// Hold the command stream open forever, reconnecting with backoff. Spawned from `run()` once the
/// edge is enrolled (it needs a box token to authenticate). `local_api` is this edge's own HTTP
/// base (e.g. `http://127.0.0.1:8788`) — commands are dispatched to it.
pub fn run(control_url: String, local_api: String, state: Arc<AppState>) {
    #[cfg(feature = "gst-linked")]
    let _ = STATE.set(state);
    #[cfg(not(feature = "gst-linked"))]
    let _ = &state; // only the gst-linked live-push path needs it
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
    std::env::var("BOX_TOKEN")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Connect and read SSE frames until the stream ends or errors. Returns Ok on a clean EOF.
fn pump(control_url: &str, local_api: &str) -> Result<(), String> {
    let Some(tok) = token() else {
        return Err("no BOX_TOKEN — not enrolled".into());
    };
    let url = format!("{control_url}/config/edge/stream");
    // A dedicated agent with a bounded READ timeout (not an overall one) so the never-ending stream
    // isn't killed mid-flight, but a dead socket is still detected between keep-alives.
    let agent = ureq::AgentBuilder::new()
        .timeout_read(Duration::from_secs(READ_TIMEOUT_SECS))
        .build();
    // Only the handshake is signed; once established the stream's integrity is TLS's job, and each
    // command's result is a separate, independently signed POST.
    let resp = crate::signer::sign(
        agent
            .get(&url)
            .set("Authorization", &format!("Bearer {tok}"))
            .set("Accept", "text/event-stream")
            .set("X-Edge-Protocol", "1"),
        "GET",
        &url,
        b"",
    )
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
    let id = v
        .get("id")
        .and_then(|x| x.as_str())
        .unwrap_or_default()
        .to_string();
    let cmd = v.get("cmd").and_then(|x| x.as_str()).unwrap_or_default();
    let args = v
        .get("args")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    log::info!("command stream: <- {cmd} (id={id})");

    let (ok, result, error) = dispatch(local_api, cmd, &args);

    // Report the outcome so the waiting cloud caller wakes.
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(30))
        .build();
    let body = serde_json::json!({ "id": id, "ok": ok, "result": result, "error": error });
    let result_url = format!("{control_url}/config/edge/command-result");
    let bytes = serde_json::to_vec(&body).unwrap_or_default();
    if let Err(e) = crate::signer::sign(
        agent
            .post(&result_url)
            .set("Authorization", &format!("Bearer {tok}"))
            .set("Content-Type", "application/json"),
        "POST",
        &result_url,
        &bytes,
    )
    .send_bytes(&bytes)
    {
        log::warn!("command stream: result POST failed for {id}: {e}");
    }
}

/// Dispatch a command to the edge's own local HTTP API — the same endpoints the app drives, so the
/// cloud path and the local path are one and the same.
///
/// The named recording commands are kept for the already-live recording node. Everything else uses
/// the **generic `http_request`** proxy (card #144): the cloud sends `{method, path, body}`, the edge
/// performs that call against `127.0.0.1:<port>` and returns `{status, content_type, body_b64}` — so
/// ANY discrete cloud→edge HTTP (probe, snapshot, tracks, get-frame, clip-delete…) rides the SSE
/// channel without a per-endpoint command. `body_b64` carries binary (a JPEG, JSON, anything) intact.
fn dispatch(
    local_api: &str,
    cmd: &str,
    args: &serde_json::Value,
) -> (bool, Option<serde_json::Value>, Option<String>) {
    match cmd {
        "start_recording" => named_post(local_api, "/recording/start", args),
        "stop_recording" => named_post(local_api, "/recording/stop", args),
        "http_request" => http_request(local_api, args),
        "start_live" => start_live(args),
        "uninstall" => uninstall(),
        other => (false, None, Some(format!("unknown command: {other}"))),
    }
}

/// Complete self-removal (card #144): the operator removed this box in the UI, so the edge stops its
/// own service and deletes itself from the host. We ack FIRST (so the cloud confirms the host is
/// being torn down), then a **detached** uninstaller — in its own session so stopping the service
/// doesn't kill it mid-delete — waits a beat and tears everything down: the service definition, the
/// binary + config + data, and the package receipt. On an on-device edge (iOS) there is nothing to
/// uninstall — the app owns the process — so it's a no-op ack.
fn uninstall() -> (bool, Option<serde_json::Value>, Option<String>) {
    log::warn!("uninstall command received — this box will stop and remove itself from the host");

    #[cfg(target_os = "macos")]
    let script = "sleep 1
launchctl bootout system/io.sentinel.edge 2>/dev/null || true
rm -f /Library/LaunchDaemons/io.sentinel.edge.plist
rm -rf /opt/sentinel-edge /etc/sentinel-edge
pkgutil --forget io.sentinel.edge 2>/dev/null || true";

    #[cfg(target_os = "linux")]
    let script = "sleep 1
systemctl stop sentinel-edge 2>/dev/null || true
systemctl disable sentinel-edge 2>/dev/null || true
rm -f /etc/systemd/system/sentinel-edge.service
systemctl daemon-reload 2>/dev/null || true
rm -rf /opt/sentinel-edge /etc/sentinel-edge";

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    let script = "";

    if script.is_empty() {
        return (
            true,
            Some(
                serde_json::json!({ "uninstalled": false, "reason": "on-device edge — nothing to uninstall" }),
            ),
            None,
        );
    }

    // `setsid` puts the uninstaller in a new session so `bootout`/`stop` killing THIS process's group
    // doesn't take the uninstaller with it. Detached from our stdio so it survives our death.
    let spawned = std::process::Command::new("sh")
        .arg("-c")
        .arg(format!(
            "setsid sh -c '{script}' </dev/null >/dev/null 2>&1 &"
        ))
        .spawn();
    match spawned {
        Ok(_) => (
            true,
            Some(serde_json::json!({ "uninstalling": true })),
            None,
        ),
        Err(e) => (
            false,
            None,
            Some(format!("could not launch uninstaller: {e}")),
        ),
    }
}

/// Live-over-outbound (card #144): dial the gateway and push the camera's fMP4 up. `args` = `{src,
/// url}` — `src` is the go2rtc stream name the browser asked for, `url` the gateway WS to dial. The
/// stream runs on its own thread+runtime until the gateway closes; this returns an immediate ack.
#[cfg(feature = "gst-linked")]
fn start_live(args: &serde_json::Value) -> (bool, Option<serde_json::Value>, Option<String>) {
    let Some(url) = args.get("url").and_then(|u| u.as_str()) else {
        return (false, None, Some("start_live: missing 'url'".into()));
    };
    let Some(src) = args.get("src").and_then(|s| s.as_str()) else {
        return (false, None, Some("start_live: missing 'src'".into()));
    };
    let Some((ip, hd)) = crate::http::go2rtc_src_to_camera(src) else {
        return (false, None, Some(format!("start_live: bad src '{src}'")));
    };
    let Some(state) = STATE.get().cloned() else {
        return (false, None, Some("start_live: edge state not ready".into()));
    };
    let (url, ip) = (url.to_string(), ip);
    // Its own current-thread runtime: one live session per outbound socket, isolated from the
    // command stream and torn down with the thread when the gateway closes.
    std::thread::spawn(move || {
        let rt = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(rt) => rt,
            Err(e) => {
                log::warn!("live-push: runtime build failed: {e}");
                return;
            }
        };
        rt.block_on(crate::live_push::run(url, ip, hd, state));
    });
    (true, Some(serde_json::json!({ "started": true })), None)
}

#[cfg(not(feature = "gst-linked"))]
fn start_live(_args: &serde_json::Value) -> (bool, Option<serde_json::Value>, Option<String>) {
    (
        false,
        None,
        Some("live requires the GStreamer backend (gst-linked)".into()),
    )
}

/// A named POST whose JSON reply is the command result (the recording commands).
fn named_post(
    local_api: &str,
    path: &str,
    args: &serde_json::Value,
) -> (bool, Option<serde_json::Value>, Option<String>) {
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(120))
        .build();
    match agent
        .post(&format!("{local_api}{path}"))
        .send_json(args.clone())
    {
        Ok(resp) => match resp.into_json::<serde_json::Value>() {
            Ok(v) => (true, Some(v), None),
            Err(e) => (false, None, Some(format!("decode: {e}"))),
        },
        Err(ureq::Error::Status(code, resp)) => (
            false,
            None,
            Some(
                resp.into_string()
                    .unwrap_or_else(|_| format!("HTTP {code}")),
            ),
        ),
        Err(e) => (false, None, Some(format!("{e}"))),
    }
}

/// The generic local-HTTP proxy. Reconstructs the edge's response as `{status, content_type,
/// body_b64}` so the cloud caller can rebuild it verbatim (bytes and status preserved).
fn http_request(
    local_api: &str,
    args: &serde_json::Value,
) -> (bool, Option<serde_json::Value>, Option<String>) {
    use base64::Engine;
    use std::io::Read;

    let method = args
        .get("method")
        .and_then(|m| m.as_str())
        .unwrap_or("GET")
        .to_uppercase();
    let Some(path) = args.get("path").and_then(|p| p.as_str()) else {
        return (false, None, Some("http_request: missing 'path'".into()));
    };
    let url = format!("{local_api}{path}");
    // 120s: a snapshot may cold-start its decoder; a clip fetch can be large.
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(120))
        .build();
    let req = agent.request(&method, &url);
    // `body` is an arbitrary JSON value (POST/PUT payload); GET/DELETE usually omit it.
    let sent = match args.get("body") {
        Some(b) if !b.is_null() => req.send_json(b.clone()),
        _ => req.call(),
    };
    // Both a 2xx and a 4xx/5xx are a *delivered* result — the cloud handler decides what a non-2xx
    // means (a probe's `auth_failed` is a 200 with a verdict; a real edge error is a 5xx body). Only
    // a transport failure (couldn't reach our own localhost) is a command error.
    let resp = match sent {
        Ok(r) => r,
        Err(ureq::Error::Status(_, r)) => r,
        Err(e) => return (false, None, Some(format!("local http: {e}"))),
    };
    let status = resp.status();
    let content_type = resp.content_type().to_string();
    let mut bytes = Vec::new();
    if let Err(e) = resp
        .into_reader()
        .take(64 * 1024 * 1024)
        .read_to_end(&mut bytes)
    {
        return (false, None, Some(format!("read body: {e}")));
    }
    let body_b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
    (
        true,
        Some(
            serde_json::json!({ "status": status, "content_type": content_type, "body_b64": body_b64 }),
        ),
        None,
    )
}
