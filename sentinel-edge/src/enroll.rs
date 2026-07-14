//! Enrollment: how a box learns whose it is (card #81).
//!
//! Installing a box used to mean pasting four arguments — two of them secrets — onto its command
//! line. A double-clicked `.pkg`/`.exe` (#82/#83) has no way to receive those, so instead: the web
//! app mints a short pairing code, and the person **standing at the appliance** (they just ran the
//! installer on it) types that code into this page. The edge redeems it with control, writes its
//! identity to `edge.env`, and restarts into it.
//!
//! The page is one self-contained file — no framework, no CDN, no fonts. It has to render on a
//! machine that has nothing installed and may not be able to reach anything but control.

use std::time::Duration;

use axum::extract::State;
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::json;

/// Is this box already somebody's? The box token is the whole of its identity.
pub fn enrolled() -> bool {
    std::env::var("BOX_TOKEN").map(|t| !t.trim().is_empty()).unwrap_or(false)
}

/// The **only** thing an unenrolled box serves. It deliberately does not build an `AppState`, and
/// that is a correctness requirement, not tidiness: `fetch_cameras` falls back to the **default
/// tenant** when it presents no box token, so a box that booted without an identity and went
/// through the normal path would happily pull down somebody else's cameras.
pub fn router(control_url: String) -> Router {
    Router::new()
        .route("/", get(page))
        .route("/enroll", post(enroll))
        .layer(tower_http::cors::CorsLayer::permissive())
        .with_state(control_url)
}

#[derive(serde::Deserialize)]
pub struct EnrollReq {
    pub code: String,
}

/// Redeem the code with control, persist what comes back, and restart into it.
pub async fn enroll(State(control_url): State<String>, Json(req): Json<EnrollReq>) -> Response {
    if enrolled() {
        return (StatusCode::CONFLICT, Json(json!({ "error": "this box is already enrolled" }))).into_response();
    }
    let code = req.code.trim().to_string();
    if code.is_empty() {
        return (StatusCode::BAD_REQUEST, Json(json!({ "error": "enter the code from the Boxes page" }))).into_response();
    }

    let result = tokio::task::spawn_blocking(move || redeem(&control_url, &code)).await;

    match result {
        Ok(Ok(())) => {
            // Restart so the whole boot path runs with an identity: pull cameras, join the tailnet,
            // start the hub. The supervisor (launchd KeepAlive / systemd Restart=always) brings us
            // back — the same contract the Boxes-page Restart button already relies on.
            std::thread::spawn(|| {
                std::thread::sleep(Duration::from_millis(600)); // let the response flush first
                log::info!("enrolled — restarting to come up as a real box");
                std::process::exit(0);
            });
            Json(json!({ "ok": true })).into_response()
        }
        Ok(Err(e)) => (StatusCode::BAD_REQUEST, Json(json!({ "error": e }))).into_response(),
        Err(e) => {
            log::error!("enroll task panicked: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": "internal error" }))).into_response()
        }
    }
}

/// The blocking half: POST the code to control, write what it returns.
fn redeem(control_url: &str, code: &str) -> Result<(), String> {
    let url = format!("{control_url}/enroll");
    let resp = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(20))
        .build()
        .post(&url)
        .send_json(json!({ "code": code }));

    let body: serde_json::Value = match resp {
        Ok(r) => r.into_json().map_err(|e| format!("control sent something unreadable: {e}"))?,
        // Control answers a bad/expired/spent code with one deliberately uninformative message.
        // Pass it through rather than inventing a more specific one we can't actually know.
        Err(ureq::Error::Status(_, r)) => {
            let msg = r
                .into_json::<serde_json::Value>()
                .ok()
                .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(str::to_string))
                .unwrap_or_else(|| "that code isn't valid".to_string());
            return Err(msg);
        }
        Err(e) => return Err(format!("couldn't reach {control_url}: {e}")),
    };

    let token = body.get("box_token").and_then(|v| v.as_str()).unwrap_or_default();
    if token.is_empty() {
        return Err("control didn't issue a token".to_string());
    }
    let hostname = body.get("hostname").and_then(|v| v.as_str()).unwrap_or_default();
    let ts_key = body.get("tailscale_key").and_then(|v| v.as_str()).unwrap_or_default();

    // CONTROL_URL is written too: it isn't a secret (it's a public hostname), but the box must
    // remember which control plane it belongs to across restarts.
    let mut updates: Vec<(&str, &str)> = vec![("CONTROL_URL", control_url), ("BOX_TOKEN", token)];
    if !hostname.is_empty() {
        updates.push(("TS_HOSTNAME", hostname));
    }
    if !ts_key.is_empty() {
        updates.push(("TS_AUTHKEY", ts_key));
    }
    crate::envfile::merge_write(&updates).map_err(|e| format!("couldn't save the box's identity: {e}"))?;
    Ok(())
}

/// The enrollment page. Served at `/` while the box has no identity — the installer opens it
/// (`open`/`start http://localhost:8788`) as its last step.
pub async fn page() -> Response {
    (
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        PAGE,
    )
        .into_response()
}

const PAGE: &str = r##"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>Connect this appliance</title>
<style>
  :root { color-scheme: light dark; }
  * { box-sizing: border-box; }
  body {
    margin: 0; min-height: 100vh; display: grid; place-items: center;
    font: 16px/1.5 -apple-system, BlinkMacSystemFont, "Segoe UI", system-ui, sans-serif;
    background: #fafafa; color: #111;
  }
  @media (prefers-color-scheme: dark) { body { background: #0b0b0c; color: #f2f2f2; } }
  main { width: 100%; max-width: 26rem; padding: 2rem; }
  h1 { font-size: 1.5rem; margin: 0 0 .5rem; letter-spacing: -.01em; }
  p  { margin: 0 0 1.5rem; opacity: .7; }
  label { display: block; font-size: .8rem; text-transform: uppercase; letter-spacing: .08em; opacity: .6; margin-bottom: .5rem; }
  input {
    width: 100%; padding: .9rem 1rem; font: 600 1.4rem/1 ui-monospace, SFMono-Regular, Menlo, monospace;
    letter-spacing: .18em; text-align: center; text-transform: uppercase;
    border: 1px solid rgba(128,128,128,.35); border-radius: .6rem; background: transparent; color: inherit;
  }
  input:focus { outline: 2px solid currentColor; outline-offset: 2px; }
  button {
    width: 100%; margin-top: 1rem; padding: .9rem 1rem; font: 600 1rem/1 inherit;
    border: 0; border-radius: .6rem; background: #111; color: #fff; cursor: pointer;
  }
  @media (prefers-color-scheme: dark) { button { background: #f2f2f2; color: #111; } }
  button:disabled { opacity: .5; cursor: default; }
  #msg { margin-top: 1rem; min-height: 1.5rem; font-size: .9rem; }
  #msg.err { color: #c0392b; }
  #msg.ok  { color: #1e8e5a; }
  @media (prefers-color-scheme: dark) { #msg.err { color: #ff6b5b; } #msg.ok { color: #4ade80; } }
</style>
</head>
<body>
<main>
  <h1>Connect this appliance</h1>
  <p>Open the Boxes page in your Sentinel account, add a box, and type the code it shows you.</p>
  <form id="f">
    <label for="code">Pairing code</label>
    <input id="code" name="code" placeholder="ABCDE-FGHIJ" autocomplete="off" autocapitalize="characters"
           spellcheck="false" autofocus required>
    <button id="go" type="submit">Connect</button>
  </form>
  <div id="msg" role="status" aria-live="polite"></div>
</main>
<script>
  const f = document.getElementById('f');
  const go = document.getElementById('go');
  const msg = document.getElementById('msg');
  f.addEventListener('submit', async (e) => {
    e.preventDefault();
    const code = document.getElementById('code').value.trim();
    if (!code) return;
    go.disabled = true;
    msg.className = '';
    msg.textContent = 'Connecting…';
    try {
      const r = await fetch('/enroll', {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ code }),
      });
      const body = await r.json().catch(() => ({}));
      if (!r.ok) throw new Error(body.error || 'That code did not work.');
      msg.className = 'ok';
      msg.textContent = 'Connected. This appliance is restarting — you can close this page and watch it come online in your account.';
    } catch (err) {
      msg.className = 'err';
      msg.textContent = err.message;
      go.disabled = false;
    }
  });
</script>
</body>
</html>
"##;
