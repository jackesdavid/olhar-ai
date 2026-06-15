//! The edge REST API. The key route is `GET /snapshot/{ip}` — a **binary** JPEG, so
//! frame-consuming services (sentinel-flow's `FrameSource`) fetch frames out of band
//! instead of base64'ing them onto an SSE stream.
use std::sync::Arc;

use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Json};
use axum::routing::{get, post};
use axum::Router;
use serde_json::json;
use tower_http::cors::CorsLayer;

use app_core::device_actions::capture_snapshot_bytes;
use app_core::go2rtc;

use crate::state::AppState;

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/devices", get(devices))
        .route("/streams", get(streams))
        .route("/snapshot/:ip", get(snapshot))
        .route("/reload", post(reload))
        .layer(CorsLayer::permissive())
        .with_state(state)
}

async fn health(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let n = state.devices.lock().unwrap().len();
    Json(json!({ "status": "ok", "service": "sentinel-edge", "devices": n, "owns_go2rtc": state.own_go2rtc }))
}

/// The cameras the edge knows about (ip + name + whether a stream is configured).
async fn devices(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let list: Vec<_> = state
        .devices
        .lock()
        .unwrap()
        .iter()
        .map(|d| {
            let has_stream = d.rtsp_url.as_deref().map(|u| !u.trim().is_empty()).unwrap_or(false);
            json!({ "ip": d.ip, "name": d.name, "has_stream": has_stream })
        })
        .collect();
    Json(list)
}

/// Playable go2rtc stream descriptors for the Live screen (player URLs).
async fn streams(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let list: Vec<_> = state
        .devices
        .lock()
        .unwrap()
        .iter()
        .filter(|d| d.rtsp_url.as_deref().map(|u| !u.trim().is_empty()).unwrap_or(false))
        .map(|d| {
            let name = go2rtc::stream_name(&d.ip);
            json!({
                "ip": d.ip,
                "name": name,
                "player_url": format!("{}/stream.html?src={name}", go2rtc::BASE),
            })
        })
        .collect();
    Json(list)
}

/// Capture and return a JPEG snapshot for the camera at `ip` (binary; the FrameSource
/// port). go2rtc fast-path with ONVIF/ffmpeg fallback — all inside `app-core`. The
/// blocking capture runs off the async worker.
async fn snapshot(State(state): State<Arc<AppState>>, Path(ip): Path<String>) -> impl IntoResponse {
    let Some(device) = state.device_by_ip(&ip) else {
        return (StatusCode::NOT_FOUND, format!("no device with ip {ip}")).into_response();
    };
    match tokio::task::spawn_blocking(move || capture_snapshot_bytes(&device)).await {
        Ok(Ok(bytes)) => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "image/jpeg"), (header::CACHE_CONTROL, "no-store")],
            Body::from(bytes),
        )
            .into_response(),
        Ok(Err(e)) => {
            log::warn!("snapshot {ip} failed: {e}");
            (StatusCode::BAD_GATEWAY, e).into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("capture task failed: {e}")).into_response(),
    }
}

/// Re-read the device list from Postgres (and restart go2rtc if owned).
async fn reload(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let n = state.reload(0);
    log::info!("devices reloaded on request — {n} device(s)");
    Json(json!({ "reloaded": n }))
}
