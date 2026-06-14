//! The proxy core: a WebSocket bridge (MSE) and an HTTP passthrough (snapshots,
//! stream list, go2rtc's player assets). Both target the resolved upstream go2rtc.
use axum::body::Body;
use axum::extract::ws::{Message as AxMsg, WebSocket};
use axum::extract::{State, WebSocketUpgrade};
use axum::extract::RawQuery;
use axum::http::{HeaderMap, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use futures::{SinkExt, StreamExt};
use std::sync::Arc;
use std::time::Duration;
use tokio_tungstenite::tungstenite::Message as TgMsg;

use crate::state::AppState;

// ---- MSE: WebSocket bridge --------------------------------------------------

/// Upgrade the client WS and bridge it to the upstream go2rtc MSE WebSocket
/// (`{ws_upstream}/api/ws?<query>`), piping both directions until either side closes.
/// Gated by `live.view` — browsers can't set WS headers, so the token rides the query
/// (`?token=<jwt>`); the upstream never sees it (we forward only go2rtc's own params).
pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    RawQuery(query): RawQuery,
) -> Response {
    let q = query.unwrap_or_default();
    let Some((token, claims)) = crate::auth::authorize_live(&state.jwt_secret, &headers, Some(&q)) else {
        return (StatusCode::UNAUTHORIZED, "live.view required").into_response();
    };
    let base = state.upstream_for(claims.tid.as_deref(), &token).await;
    let upstream = format!("{}/api/ws?{}", AppState::ws_origin(&base), strip_token(&q));
    ws.on_upgrade(move |client| bridge_ws(client, upstream))
}

/// Drop our `token` query param so it's never forwarded to the upstream go2rtc.
fn strip_token(query: &str) -> String {
    query
        .split('&')
        .filter(|p| !p.is_empty() && !p.starts_with("token="))
        .collect::<Vec<_>>()
        .join("&")
}

/// Connect to the upstream go2rtc WS and pipe frames both ways. Best-effort: any
/// error (connect timeout, upstream close) ends the bridge and closes the client.
async fn bridge_ws(client: WebSocket, upstream: String) {
    let connect = tokio_tungstenite::connect_async(&upstream);
    let (up_stream, _resp) = match tokio::time::timeout(Duration::from_secs(8), connect).await {
        Ok(Ok(ok)) => ok,
        Ok(Err(e)) => {
            log::warn!("upstream WS connect failed ({upstream}): {e}");
            return;
        }
        Err(_) => {
            log::warn!("upstream WS connect timed out: {upstream}");
            return;
        }
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

/// go2rtc API paths that must NEVER be exposed publicly: they leak the camera RTSP
/// credentials / full config, or allow inspecting the box. The gateway is a public
/// edge, so we block these even though the gateway only forwards GET. The player needs
/// only `/api/frame.jpeg`, `/api/ws` (its own route), and the static UI — all allowed.
fn is_blocked(path: &str) -> bool {
    const BLOCKED: &[&str] = &["/api/streams", "/api/config", "/api/log", "/api/exec"];
    path == "/api" || path == "/api/" || BLOCKED.iter().any(|p| path == *p || path.starts_with(&format!("{p}/")))
}

/// Live media that carries actual frames — gated by `live.view`. Player UI assets
/// (stream.html, JS/CSS) are NOT gated: they carry no video, and the stream itself
/// flows through these endpoints + `/api/ws`, which are all gated.
fn is_live_media(path: &str) -> bool {
    path == "/api/frame.jpeg" || path == "/api/frame.mp4" || path.starts_with("/api/stream")
}

/// Stream any other GET through to the upstream go2rtc, preserving path + query
/// (snapshots `/api/frame.jpeg`, the player `/stream.html` + assets). No buffering.
/// Credential-leaking go2rtc endpoints are blocked (see [`is_blocked`]); live media is
/// gated by `live.view` (see [`is_live_media`]).
pub async fn passthrough(State(state): State<Arc<AppState>>, headers: HeaderMap, uri: Uri) -> Response {
    if is_blocked(uri.path()) {
        return (StatusCode::FORBIDDEN, "blocked").into_response();
    }
    // Live media is gated + routed to the requester's tenant box; the (identical) player
    // UI assets come from the static fallback upstream.
    let base = if is_live_media(uri.path()) {
        let Some((token, claims)) = crate::auth::authorize_live(&state.jwt_secret, &headers, uri.query()) else {
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
            if s.is_empty() { uri.path().to_string() } else { format!("{}?{}", uri.path(), s) }
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
    for h in [axum::http::header::CONTENT_TYPE, axum::http::header::CACHE_CONTROL] {
        if let Some(v) = resp.headers().get(&h) {
            if let Ok(v) = axum::http::HeaderValue::from_bytes(v.as_bytes()) {
                headers.insert(h, v);
            }
        }
    }
    let body = Body::from_stream(resp.bytes_stream());
    (status, headers, body).into_response()
}
