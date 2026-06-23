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
        .route("/recording/start", post(recording_start))
        .route("/recording/stop", post(recording_stop))
        .route("/recording/:clip_id", get(recording_get))
        .layer(CorsLayer::permissive())
        .with_state(state)
}

async fn health(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let n = state.devices.lock().unwrap().len();
    Json(
        json!({ "status": "ok", "service": "sentinel-edge", "devices": n, "owns_go2rtc": state.own_go2rtc }),
    )
}

/// The cameras the edge knows about (ip + name + whether a stream is configured).
async fn devices(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let list: Vec<_> = state
        .devices
        .lock()
        .unwrap()
        .iter()
        .map(|d| {
            let has_stream = d
                .rtsp_url
                .as_deref()
                .map(|u| !u.trim().is_empty())
                .unwrap_or(false);
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
        .filter(|d| {
            d.rtsp_url
                .as_deref()
                .map(|u| !u.trim().is_empty())
                .unwrap_or(false)
        })
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
            [
                (header::CONTENT_TYPE, "image/jpeg"),
                (header::CACHE_CONTROL, "no-store"),
            ],
            Body::from(bytes),
        )
            .into_response(),
        Ok(Err(e)) => {
            log::warn!("snapshot {ip} failed: {e}");
            (StatusCode::BAD_GATEWAY, e).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("capture task failed: {e}"),
        )
            .into_response(),
    }
}

/// Re-read the device list from Postgres (and restart go2rtc if owned).
async fn reload(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let n = state.reload(0);
    log::info!("devices reloaded on request — {n} device(s)");
    Json(json!({ "reloaded": n }))
}

// ---- Recording routes (called by sentinel-flow workflow nodes) --------------

#[derive(serde::Deserialize)]
struct StartRecordingReq {
    camera_ip: String,
    #[serde(default)]
    pre_roll_secs: Option<u32>,
}

async fn recording_start(
    State(state): State<Arc<AppState>>,
    Json(body): Json<StartRecordingReq>,
) -> impl IntoResponse {
    let mgr = state.recording.clone();
    let pre_roll = body.pre_roll_secs.unwrap_or(0);
    match tokio::task::spawn_blocking(move || mgr.start(&body.camera_ip, pre_roll)).await {
        Ok(Ok(clip_id)) => Json(json!({ "clip_id": clip_id })).into_response(),
        Ok(Err(e)) => (StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("task: {e}")).into_response(),
    }
}

#[derive(serde::Deserialize)]
struct StopRecordingReq {
    camera_ip:  String,
    #[serde(default)]
    clip_id:    Option<String>,
}

async fn recording_stop(
    State(state): State<Arc<AppState>>,
    Json(body): Json<StopRecordingReq>,
) -> impl IntoResponse {
    let mgr = state.recording.clone();
    let result = tokio::task::spawn_blocking(move || {
        if let Some(id) = body.clip_id.filter(|s| !s.is_empty()) {
            mgr.stop(&id)
        } else {
            mgr.stop_by_ip(&body.camera_ip)
                .unwrap_or_else(|| Err(format!("no active session for {}", body.camera_ip)))
        }
    })
    .await;

    match result {
        Ok(Ok(meta)) => {
            // Push the clip manifest to control so the DB + browsers know about it.
            let control_url = state.control_url.clone();
            let payload = serde_json::json!({
                "camera_ip": meta.camera_ip,
                "clip_id": meta.clip_id,
                "duration_secs": meta.duration_secs,
                "size_bytes": meta.size_bytes,
                "started_at": meta.started_at,
            });
            tokio::task::spawn_blocking(move || {
                let url = format!("{control_url}/config/recordings/clip-metadata");
                let mut req = ureq::AgentBuilder::new()
                    .timeout(std::time::Duration::from_secs(10))
                    .build()
                    .post(&url)
                    .set("Content-Type", "application/json");
                if let Ok(t) = std::env::var("BOX_TOKEN") {
                    let t = t.trim();
                    if !t.is_empty() {
                        req = req.set("Authorization", &format!("Bearer {t}"));
                    }
                }
                if let Err(e) = req.send_string(&payload.to_string()) {
                    log::warn!("clip push failed: {e}");
                }
            });
            Json(json!({
                "clip_id": meta.clip_id,
                "duration_secs": meta.duration_secs,
                "size_bytes": meta.size_bytes,
            }))
            .into_response()
        }
        Ok(Err(e)) => (StatusCode::NOT_FOUND, e).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("task: {e}")).into_response(),
    }
}

/// Serve the raw clip file (used by control's download proxy).
async fn recording_get(
    State(state): State<Arc<AppState>>,
    Path(clip_id): Path<String>,
) -> impl IntoResponse {
    let mgr = state.recording.clone();
    match tokio::task::spawn_blocking(move || mgr.clip_path(&clip_id)).await {
        Ok(Some(path)) => {
            match tokio::fs::read(&path).await {
                Ok(bytes) => (
                    StatusCode::OK,
                    [(header::CONTENT_TYPE, "video/mp4")],
                    Body::from(bytes),
                )
                    .into_response(),
                Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("read: {e}")).into_response(),
            }
        }
        Ok(None) => (StatusCode::NOT_FOUND, "clip not found").into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("task: {e}")).into_response(),
    }
}
