//! The proxy core: a WebSocket bridge (MSE) and an HTTP passthrough (snapshots,
//!
//! migration shippable: the wire contract is identical, so a box serving either one is
//! indistinguishable to the player.
use axum::body::Body;
use axum::extract::ws::{Message as AxMsg, WebSocket};
use axum::extract::RawQuery;
use axum::extract::{State, WebSocketUpgrade};
use axum::http::{HeaderMap, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use futures::{SinkExt, StreamExt};
use std::sync::Arc;
use std::time::Duration;
use tokio_tungstenite::tungstenite::Message as TgMsg;

use crate::state::AppState;

// ---- MSE: WebSocket bridge --------------------------------------------------

/// Upgrade the client WS and bridge it to the box's MSE WebSocket (`.../api/ws?<query>`), piping
/// Gated by `live.view` — browsers can't set WS headers, so the token rides the query
pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    RawQuery(query): RawQuery,
) -> Response {
    let q = query.unwrap_or_default();
    let Some((token, claims)) = crate::auth::authorize_live(&state.jwt_secret, &headers, Some(&q))
    else {
        return (StatusCode::UNAUTHORIZED, "live.view required").into_response();
    };
    let params = strip_token(&q);
    let src = param(&q, "src");

    // Live-over-outbound first (card #144): if the gateway has a public WS address, ask control to
    // set up the stream. When the tenant's edge is on the SSE command channel, control tells it to
    // dial us back — no tailnet needed. Otherwise (or if anything fails) we fall through to the
    // tailnet path below, unchanged.
    if !state.public_ws_url.is_empty() {
        let session = uuid::Uuid::new_v4().to_string();
        let callback = format!(
            "{}/edge/live/{session}",
            state.public_ws_url.trim_end_matches('/')
        );
        if let crate::state::LiveStart::Sse = state.live_start(&token, &src, &callback).await {
            let (tx, rx) = futures::channel::oneshot::channel();
            state
                .edge_sessions
                .lock()
                .unwrap()
                .insert(session.clone(), tx);
            return ws.on_upgrade(move |client| bridge_outbound(client, state, session, rx));
        }
    }

    // Tailnet path (existing): the gateway dials the edge's MSE server (its own GStreamer one first,
    let base = state.upstream_for(claims.tid.as_deref(), &token).await;
    let mut candidates = Vec::new();
    if let Some(edge) = state.live_edge_ws(&base) {
        candidates.push(format!("{edge}/api/ws?{params}"));
    }
    candidates.push(format!("{}/api/ws?{}", AppState::ws_origin(&base), params));
    ws.on_upgrade(move |client| bridge_ws(client, state, candidates))
}

/// One query param's value (empty if absent).
fn param(query: &str, key: &str) -> String {
    let pfx = format!("{key}=");
    query
        .split('&')
        .find_map(|p| p.strip_prefix(&pfx))
        .map(|v| v.to_string())
        .unwrap_or_default()
}

/// The edge dials this to deliver a live-over-outbound stream. It hands its upgraded socket to the
/// waiting browser bridge via the session rendezvous. An unknown/expired session (or a duplicate)
/// finds no waiter and the socket is simply dropped (closed).
pub async fn edge_live(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
    axum::extract::Path(session): axum::extract::Path<String>,
) -> Response {
    let Some(tx) = state.edge_sessions.lock().unwrap().remove(&session) else {
        return (StatusCode::NOT_FOUND, "no such live session").into_response();
    };
    ws.on_upgrade(move |edge| async move {
        // Hand the edge socket to the browser bridge. If it already gave up (timed out), drop it.
        if tx.send(edge).is_err() {
            log::warn!("edge-live: session {session} had no waiter");
        }
    })
}

/// The browser side of live-over-outbound: wait for the edge to dial in (bounded), then bridge the
/// two sockets. On timeout, clean up the rendezvous and close the browser.
async fn bridge_outbound(
    client: WebSocket,
    state: Arc<AppState>,
    session: String,
    rx: futures::channel::oneshot::Receiver<WebSocket>,
) {
    let edge = match tokio::time::timeout(Duration::from_secs(10), rx).await {
        Ok(Ok(edge)) => edge,
        _ => {
            state.edge_sessions.lock().unwrap().remove(&session);
            log::warn!("edge-live: session {session} — edge did not dial in within 10s");
            return;
        }
    };
    log::info!("WS bridge open (outbound) → edge session {session}");
    bridge_two_ws(client, edge).await;
    log::info!("WS bridge closed (outbound) → edge session {session}");
}

/// Pipe two axum WebSockets both ways until either closes — the browser↔edge bridge for the
/// outbound path (both sides are inbound-to-the-gateway sockets, unlike [`bridge_ws`]).
async fn bridge_two_ws(a: WebSocket, b: WebSocket) {
    let (mut a_tx, mut a_rx) = a.split();
    let (mut b_tx, mut b_rx) = b.split();
    let a2b = async {
        while let Some(Ok(msg)) = a_rx.next().await {
            if matches!(msg, AxMsg::Close(_)) || b_tx.send(msg).await.is_err() {
                break;
            }
        }
        let _ = b_tx.close().await;
    };
    let b2a = async {
        while let Some(Ok(msg)) = b_rx.next().await {
            if matches!(msg, AxMsg::Close(_)) || a_tx.send(msg).await.is_err() {
                break;
            }
        }
        let _ = a_tx.close().await;
    };
    tokio::select! { _ = a2b => {}, _ = b2a => {} }
}

fn strip_token(query: &str) -> String {
    query
        .split('&')
        .filter(|p| !p.is_empty() && !p.starts_with("token="))
        .collect::<Vec<_>>()
        .join("&")
}

/// error (connect timeout, upstream close) ends the bridge and closes the client.
async fn bridge_ws(client: WebSocket, state: Arc<AppState>, candidates: Vec<String>) {
    let last = candidates.len().saturating_sub(1);
    let mut connected = None;
    for (i, url) in candidates.iter().enumerate() {
        let is_fallback = i == last;
        // A box that cannot serve the preferred upstream must not pay for the attempt on every
        // reconnect, so a failure is remembered and skipped until the note expires.
        if !is_fallback && state.live_edge_is_dead(url) {
            continue;
        }
        // Not every box even exposes the edge's port on its tailnet, and there the connect does not
        // fail — it hangs. Give the preferred upstream a short leash so falling back is quick; only
        // the real fallback, which has nowhere else to go, gets the patient timeout.
        let budget = if is_fallback {
            Duration::from_secs(8)
        } else {
            Duration::from_secs(2)
        };
        let connect = tokio_tungstenite::connect_async(url);
        match tokio::time::timeout(budget, connect).await {
            Ok(Ok(ok)) => {
                connected = Some((url.clone(), ok.0));
                break;
            }
            Ok(Err(e)) => log::warn!("upstream WS connect failed ({url}): {e}"),
            Err(_) => log::warn!("upstream WS connect timed out: {url}"),
        }
        if !is_fallback {
            state.live_edge_mark_dead(url);
        }
    }
    let Some((upstream, up_stream)) = connected else {
        return;
    };
    log::info!("WS bridge open → {upstream}");

    let (mut up_tx, mut up_rx) = up_stream.split();
    let (mut cl_tx, mut cl_rx) = client.split();

    // client → upstream
    let c2u = async {
        while let Some(Ok(msg)) = cl_rx.next().await {
            let fwd = match msg {
                AxMsg::Text(t) => TgMsg::Text(t.into()),
                AxMsg::Binary(b) => TgMsg::Binary(b.into()),
                AxMsg::Ping(b) => TgMsg::Ping(b.into()),
                AxMsg::Pong(b) => TgMsg::Pong(b.into()),
                AxMsg::Close(_) => break,
            };
            if up_tx.send(fwd).await.is_err() {
                break;
            }
        }
        let _ = up_tx.close().await;
    };

    // upstream → client
    let u2c = async {
        while let Some(Ok(msg)) = up_rx.next().await {
            let fwd = match msg {
                TgMsg::Text(t) => AxMsg::Text(t.to_string()),
                TgMsg::Binary(b) => AxMsg::Binary(b.to_vec()),
                TgMsg::Ping(b) => AxMsg::Ping(b.to_vec()),
                TgMsg::Pong(b) => AxMsg::Pong(b.to_vec()),
                TgMsg::Close(_) => break,
                TgMsg::Frame(_) => continue, // raw frames aren't surfaced by axum
            };
            if cl_tx.send(fwd).await.is_err() {
                break;
            }
        }
        let _ = cl_tx.close().await;
    };

    // Run both directions; finish when either completes (one side closed).
    tokio::select! {
        _ = c2u => {},
        _ = u2c => {},
    }
    log::info!("WS bridge closed → {upstream}");
}

// ---- HTTP passthrough -------------------------------------------------------

/// credentials / full config, or allow inspecting the box. The gateway is a public
/// edge, so we block these even though the gateway only forwards GET. The player needs
/// only `/api/frame.jpeg`, `/api/ws` (its own route), and the static UI — all allowed.
/// Paths this gateway will forward upstream — an ALLOWLIST, not a denylist.
///
/// This is a public, internet-facing proxy: it is scanned continuously for `/.env`, `/.git/config`,
/// `phpinfo.php` and friends. It previously forwarded *every* unmatched path upstream and only
/// unauthenticated. Nothing was exposed only because the upstream happened to be unreachable —
/// security by broken config. Inverting it means a path has to be named here to leave the gateway.
///
/// Live media (frames) is additionally gated by `live.view` — see [`is_live_media`]. The player
/// assets carry no video, so they pass ungated but still only if they match.
fn is_allowed(path: &str) -> bool {
    // Refused FIRST, before the live-media prefix below. `is_live_media` matches the prefix
    // contains the RTSP URLs *including camera credentials*. The old denylist happened to catch
    // it because it ran before that prefix test; keeping it explicit means the ordering is stated,
    // not accidental.
    const NEVER: &[&str] = &["/api/streams", "/api/config", "/api/log", "/api/exec"];
    if path == "/api"
        || path == "/api/"
        || NEVER
            .iter()
            .any(|p| path == *p || path.starts_with(&format!("{p}/")))
    {
        return false;
    }
    if is_live_media(path) {
        return true;
    }
    if path == "/stream.html" || path == "/webrtc.html" {
        return true;
    }
    matches!(
        std::path::Path::new(path)
            .extension()
            .and_then(|e| e.to_str()),
        Some("js" | "css" | "ico" | "png" | "svg" | "woff2" | "map")
    )
}

/// Live media that carries actual frames — gated by `live.view`. Player UI assets
/// (stream.html, JS/CSS) are NOT gated: they carry no video, and the stream itself
/// flows through these endpoints + `/api/ws`, which are all gated.
fn is_live_media(path: &str) -> bool {
    path == "/api/frame.jpeg" || path == "/api/frame.mp4" || path.starts_with("/api/stream")
}

/// (snapshots `/api/frame.jpeg`, the player `/stream.html` + assets). No buffering.
/// gated by `live.view` (see [`is_live_media`]).
pub async fn passthrough(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    uri: Uri,
) -> Response {
    if !is_allowed(uri.path()) {
        // 404, not 403: a refusal that distinguishes "exists but forbidden" from "no such thing"
        // hands a scanner a map. Logged at debug so a legitimate asset we forgot is still findable.
        log::debug!("passthrough refused (not allowlisted): {}", uri.path());
        return (StatusCode::NOT_FOUND, "not found").into_response();
    }
    // Live media is gated + routed to the requester's tenant box; the (identical) player
    // UI assets come from the static fallback upstream.
    let base = if is_live_media(uri.path()) {
        let Some((token, claims)) =
            crate::auth::authorize_live(&state.jwt_secret, &headers, uri.query())
        else {
            return (StatusCode::UNAUTHORIZED, "live.view required").into_response();
        };
        state.upstream_for(claims.tid.as_deref(), &token).await
    } else {
        state.resolve_upstream().to_string()
    };
    // Forward path + query, but never leak our `token` param to the upstream.
    let fwd = match uri.query() {
        Some(q) => {
            let s = strip_token(q);
            if s.is_empty() {
                uri.path().to_string()
            } else {
                format!("{}?{}", uri.path(), s)
            }
        }
        None => uri.path().to_string(),
    };
    let url = format!("{base}{fwd}");

    let resp = match state.http.get(&url).send().await {
        Ok(r) => r,
        Err(e) => {
            log::warn!("upstream GET failed ({url}): {e}");
            return (StatusCode::BAD_GATEWAY, format!("edge unreachable: {e}")).into_response();
        }
    };

    let status = StatusCode::from_u16(resp.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let mut headers = HeaderMap::new();
    for h in [
        axum::http::header::CONTENT_TYPE,
        axum::http::header::CACHE_CONTROL,
    ] {
        if let Some(v) = resp.headers().get(&h) {
            if let Ok(v) = axum::http::HeaderValue::from_bytes(v.as_bytes()) {
                headers.insert(h, v);
            }
        }
    }
    let body = Body::from_stream(resp.bytes_stream());
    (status, headers, body).into_response()
}

#[cfg(test)]
mod allowlist_tests {
    use super::{is_allowed, is_live_media};

    /// The paths a real scanner hit this gateway with (from production logs) must never leave it.
    #[test]
    fn scanner_probes_are_refused() {
        for p in [
            "/.env",
            "/.env.supabase",
            "/.env.neon",
            "/.git/config",
            "/phpinfo.php",
            "/admin/phpinfo.php",
            "/_profiler/phpinfo",
            "/server-status.php",
            "/webroot/index.php/_environment",
            "/mail/phpinfo.php",
            "/debug.php",
            "/i.php",
        ] {
            assert!(!is_allowed(p), "{p} must not be forwarded upstream");
        }
    }

    #[test]
    fn credential_leaking_endpoints_are_refused() {
        for p in [
            "/api",
            "/api/",
            "/api/streams",
            "/api/config",
            "/api/log",
            "/api/exec",
        ] {
            assert!(!is_allowed(p), "{p} must not be forwarded upstream");
        }
    }

    /// Live media still passes the allowlist — it is separately gated by `live.view`.
    #[test]
    fn live_media_is_allowed_and_gated() {
        for p in ["/api/frame.jpeg", "/api/frame.mp4", "/api/stream.m3u8"] {
            assert!(is_allowed(p), "{p} must reach the live gate");
            assert!(is_live_media(p), "{p} must require live.view");
        }
    }

    /// The player page + its assets pass ungated: they carry no video.
    #[test]
    fn player_assets_are_allowed() {
        for p in [
            "/stream.html",
            "/main.js",
            "/style.css",
            "/favicon.ico",
            "/logo.svg",
        ] {
            assert!(is_allowed(p), "{p} is a player asset and should pass");
            assert!(
                !is_live_media(p),
                "{p} carries no video, must not need live.view"
            );
        }
    }
}
