//! The edge REST API. The key route is `GET /snapshot/{ip}` — a **binary** JPEG, so
//! frame-consuming services (sentinel-flow's `FrameSource`) fetch frames out of band
//! instead of base64'ing them onto an SSE stream.
use std::sync::Arc;

use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Json};
use axum::routing::{get, post};
use axum::Router;
use serde_json::json;
use tower_http::cors::CorsLayer;

use app_core::go2rtc;

use crate::state::AppState;

pub fn router(state: Arc<AppState>) -> Router {
    let router = Router::new()
        .route("/health", get(health))
        .route("/devices", get(devices))
        .route("/streams", get(streams))
        .route("/snapshot/:ip", get(snapshot))
        .route("/probe/:ip", post(probe))
        .route("/tracks/:ip", get(tracks))
        .route("/reload", post(reload))
        .route("/recordings", get(recordings_list))
        .route("/storage/stats", get(storage_stats))
        .route("/recording/start", post(recording_start))
        .route("/recording/stop", post(recording_stop))
        .route(
            "/recording/:clip_id",
            get(recording_get).delete(recording_delete),
        )
        .layer(CorsLayer::permissive())
        .with_state(state);
    #[cfg(feature = "gst-linked")]
    let router = router.route("/api/ws", get(mse_ws));
    router
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

#[derive(serde::Deserialize)]
struct ProbeReq {
    username: String,
    password: String,
}

/// Prove a camera's credentials **without persisting them** (card #88).
///
/// The old wizard saved first and then polled for a snapshot until one appeared. That inverted
/// the order — unproven creds landed in `go2rtc.yaml`, aiming a reconnect loop at the camera —
/// and the poll retried hard enough to trip a Tapo's RTSP lockout, so a single typo could take
/// a *working* camera offline until someone power-cycled it. Worse, every failure surfaced as
/// "wrong password", which sent users re-typing correct creds and deepening the lock.
///
/// So: probe once, persist nothing, and name the actual cause. Only a verified `ok` should be
/// followed by `POST /devices/:ip/credentials`.
async fn probe(
    State(state): State<Arc<AppState>>,
    Path(ip): Path<String>,
    Json(req): Json<ProbeReq>,
) -> impl IntoResponse {
    let Some(device) = state.device_by_ip(&ip) else {
        return (StatusCode::NOT_FOUND, format!("no device with ip {ip}")).into_response();
    };
    match tokio::task::spawn_blocking(move || probe_device(&device, &req.username, &req.password))
        .await
    {
        Ok(v) => Json(v).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("probe task failed: {e}"),
        )
            .into_response(),
    }
}

/// Resolve a stream URL for `device`, probe it once, and — only once the creds are proven —
/// pull a single frame as visual proof for the user.
fn probe_device(device: &app_core::devices::Device, user: &str, pass: &str) -> serde_json::Value {
    use app_core::rtsp_probe::{self, ProbeStatus};

    // Prefer the substream: same credentials, a fraction of the bandwidth of a 4K keyframe.
    let cached = device
        .rtsp_sub_url
        .as_deref()
        .or(device.rtsp_url.as_deref())
        .map(str::trim)
        .filter(|u| !u.is_empty())
        .map(str::to_string);

    // A camera whose creds were never set has no cached URL yet, so ONVIF is the only way to
    // learn its stream path. ONVIF authenticates too, so a rejection there is already a verdict.
    let url = match cached {
        Some(u) => u,
        None => {
            let target = if device.xaddr.trim().is_empty() {
                device.ip.clone()
            } else {
                device.xaddr.clone()
            };
            match app_core::onvif::client::resolve_stream_uris(&target, user, pass) {
                Ok((main, sub)) => sub.unwrap_or(main),
                Err(e) => {
                    let rejected = e.contains("credentials likely rejected");
                    let status = if rejected {
                        ProbeStatus::AuthFailed
                    } else {
                        ProbeStatus::Error
                    };
                    let detail = if rejected {
                        format!("Camera rejected the credentials over ONVIF: {e}")
                    } else {
                        format!("Couldn't ask the camera which streams it has: {e}")
                    };
                    return json!({ "status": status, "detail": detail, "snapshot": serde_json::Value::Null });
                }
            }
        }
    };

    let verdict = rtsp_probe::probe(&url, user, pass);
    log::info!(
        "probe {} → {:?}: {}",
        device.ip,
        verdict.status,
        verdict.detail
    );

    // The frame is proof for the user's eyes, not part of the verdict — a camera that
    // authenticated but won't hand over a frame is still a credential success.
    let snapshot = match verdict.status {
        ProbeStatus::Ok => app_core::onvif::client::snapshot_from_rtsp(&url, user, pass)
            .map(|b| {
                use base64::Engine;
                base64::engine::general_purpose::STANDARD.encode(b)
            })
            .map_err(|e| log::warn!("probe {}: creds ok but no proof frame: {e}", device.ip))
            .ok(),
        _ => None,
    };

    json!({ "status": verdict.status, "detail": verdict.detail, "snapshot": snapshot })
}

/// Capture and return a JPEG snapshot for the camera at `ip` (binary; the FrameSource port).
///
/// **ONE path: the warm buffer** (card #138, Stage 2). Every snapshot is the frame the supervised
/// per-camera decoder (`capture.rs`) is already producing — the same frame motion, detection and the
/// hub see. If the camera has no decoder yet we start one and wait briefly for its first frame,
/// after which it stays warm and subsequent snapshots are free.
///
/// There are deliberately **no fallbacks** here anymore. The old chain (go2rtc `/api/frame.jpeg` →
/// ONVIF `GetSnapshotUri` → a one-shot RTSP grab) meant four ways to answer one question, two of
/// them spawning **ffmpeg** — go2rtc does so internally for `frame.jpeg` (~490 ms/grab). Collapsing
/// to the warm buffer removes that duplication, and if the decoder cannot produce a frame the camera
/// is unusable for live/recording/detection anyway, so reporting the failure is the honest answer.
///
/// The credential probe (`/probe/:ip`) is a genuinely different job — validating arbitrary
/// credentials against a camera that may not be configured at all — and keeps its own direct grab.
async fn snapshot(State(state): State<Arc<AppState>>, Path(ip): Path<String>) -> impl IntoResponse {
    let Some(device) = state.device_by_ip(&ip) else {
        return (StatusCode::NOT_FOUND, format!("no device with ip {ip}")).into_response();
    };
    // Start the decoder on the SAME stream the frame hub would choose, so a snapshot never spins up
    // a second decoder on the wrong stream (`ensure` is idempotent — first call wins per camera).
    let hd = !device.is_unified()
        && state
            .recording
            .config
            .face_capture_stream
            .eq_ignore_ascii_case("hd");
    crate::capture::ensure(&ip, hd, &state.recording.config.media_backend);

    // Wait out a cold start (RTSP connect + first decoded frame) before giving up.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
    loop {
        if let Some(jpeg) = crate::capture::latest_fresh(&ip, std::time::Duration::from_secs(3)) {
            return (
                StatusCode::OK,
                [
                    (header::CONTENT_TYPE, "image/jpeg"),
                    (header::CACHE_CONTROL, "no-store"),
                ],
                Body::from(jpeg.to_vec()),
            )
                .into_response();
        }
        if std::time::Instant::now() >= deadline {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    }
    log::warn!("snapshot {ip}: decoder produced no frame within 8s");
    (
        StatusCode::BAD_GATEWAY,
        format!("no frame from {ip} — the camera's decoder produced nothing"),
    )
        .into_response()
}

/// The tracker's live "state of the world" for camera `ip` (card: edge tracking) — every
/// currently-confirmed track across every job (class) watching this camera, from the shared
/// snapshot each job's worker publishes every detection cycle. No live computation here; this is
/// just a read of the last-known state, so it's cheap even under repeated polling.
async fn tracks(State(state): State<Arc<AppState>>, Path(ip): Path<String>) -> impl IntoResponse {
    let out: Vec<_> = state
        .live_tracks
        .lock()
        .unwrap()
        .iter()
        .filter(|((track_ip, _), _)| track_ip == &ip)
        .flat_map(|(_, tracks)| tracks.clone())
        .collect();
    Json(out)
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
        Ok(Ok((clip_id, new))) => Json(json!({ "clip_id": clip_id, "new": new })).into_response(),
        Ok(Err(e)) => (StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("task: {e}")).into_response(),
    }
}

#[derive(serde::Deserialize)]
struct StopRecordingReq {
    camera_ip: String,
    #[serde(default)]
    clip_id: Option<String>,
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
            // Push the clip manifest to control so the DB + browsers know about it. Control
            // answers with a presigned PUT when the tenant's plan sells cloud recording; that
            // ticket is the only way this box can reach durable storage (it holds no cloud
            // credential), so hand it to the upload worker.
            let control_url = state.control_url.clone();
            let uploads = state.uploads.clone();
            let clip_id = meta.clip_id.clone();
            let clip_path = state.recording.clip_path(&clip_id);
            let delete_local = state.recording.config.delete_local_after_upload;
            let payload = serde_json::json!({
                "camera_ip":     meta.camera_ip,
                "clip_id":       meta.clip_id,
                "friendly_name": meta.friendly_name,
                "duration_secs": meta.duration_secs,
                "size_bytes":    meta.size_bytes,
                "started_at":    meta.started_at,
            });
            tokio::task::spawn_blocking(move || {
                let Some(url) = push_manifest(&control_url, &payload) else {
                    return; // no cloud recording on this plan, or the push failed
                };
                let Some(path) = clip_path else {
                    log::warn!("clip {clip_id}: file vanished before upload");
                    return;
                };
                crate::clip_upload::enqueue(
                    &uploads,
                    crate::clip_upload::Job {
                        clip_id,
                        path,
                        url,
                        delete_local,
                    },
                );
            });
            Json(json!({
                "clip_id": meta.clip_id,
                "duration_secs": meta.duration_secs,
                "size_bytes": meta.size_bytes,
            }))
            .into_response()
        }
        Ok(Err(e)) => {
            let (status, kind) = if e.contains("no active session") {
                (StatusCode::NOT_FOUND, "not_found")
            } else {
                (StatusCode::INTERNAL_SERVER_ERROR, "error")
            };
            log::warn!("recording stop [{kind}]: {e}");
            (status, e).into_response()
        }
        Err(e) => {
            log::warn!("recording stop [task panic]: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, format!("task: {e}")).into_response()
        }
    }
}

/// POST the clip manifest to control (box-authed, blocking). Returns the presigned PUT URL when
/// control offers one — it does only for a tenant whose plan sells cloud recording. A failure
/// here costs nothing but durability: the clip is already on local disk and served from there.
fn push_manifest(control_url: &str, payload: &serde_json::Value) -> Option<String> {
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
    let resp = match req.send_string(&payload.to_string()) {
        Ok(r) => r,
        Err(e) => {
            log::warn!("clip push failed: {e}");
            return None;
        }
    };
    resp.into_json::<serde_json::Value>()
        .ok()?
        .get("upload")?
        .get("url")?
        .as_str()
        .map(str::to_string)
}

/// Serve the raw clip file (used by control's download proxy).
async fn recording_get(
    State(state): State<Arc<AppState>>,
    Path(clip_id): Path<String>,
) -> impl IntoResponse {
    let mgr = state.recording.clone();
    match tokio::task::spawn_blocking(move || mgr.clip_path(&clip_id)).await {
        Ok(Some(path)) => match tokio::fs::read(&path).await {
            Ok(bytes) => (
                StatusCode::OK,
                [(header::CONTENT_TYPE, "video/mp4")],
                Body::from(bytes),
            )
                .into_response(),
            Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("read: {e}")).into_response(),
        },
        Ok(None) => (StatusCode::NOT_FOUND, "clip not found").into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("task: {e}")).into_response(),
    }
}

/// Delete a clip by id — removes the .mp4 and sidecar .json from disk.
async fn recording_delete(
    State(state): State<Arc<AppState>>,
    Path(clip_id): Path<String>,
) -> impl IntoResponse {
    let mgr = state.recording.clone();
    match tokio::task::spawn_blocking(move || mgr.delete_clip(&clip_id)).await {
        Ok(Ok(())) => Json(json!({ "ok": true })).into_response(),
        Ok(Err(e)) if e.contains("not found") => (StatusCode::NOT_FOUND, e).into_response(),
        Ok(Err(e)) => (StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("task: {e}")).into_response(),
    }
}

// ---- recordings list (server-side filter/sort/paginate) --------------------

#[derive(serde::Deserialize)]
struct RecordingsListQuery {
    camera_ip: Option<String>,
    sort: Option<String>,
    dir: Option<String>,
    limit: Option<usize>,
    offset: Option<usize>,
    from: Option<String>,
    to: Option<String>,
}

async fn recordings_list(
    State(state): State<Arc<AppState>>,
    Query(q): Query<RecordingsListQuery>,
) -> impl IntoResponse {
    use crate::recording::ListParams;
    let params = ListParams {
        camera_ip: q.camera_ip,
        sort: q.sort.unwrap_or_else(|| "started_at".into()),
        dir: q.dir.unwrap_or_else(|| "desc".into()),
        limit: q.limit.unwrap_or(50).min(200),
        offset: q.offset.unwrap_or(0),
        from: q.from,
        to: q.to,
    };
    let mgr = state.recording.clone();
    match tokio::task::spawn_blocking(move || mgr.list_clips(&params)).await {
        Ok(page) => Json(json!({ "total": page.total, "items": page.items })).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("task: {e}")).into_response(),
    }
}

async fn storage_stats(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let mgr = state.recording.clone();
    match tokio::task::spawn_blocking(move || mgr.storage_stats()).await {
        Ok(s) => Json(json!({
            "used_bytes":  s.used_bytes,
            "max_bytes":   s.max_bytes,
            "clip_count":  s.clip_count,
            "path":        s.path,
            "pct":         s.pct,
        }))
        .into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("task: {e}")).into_response(),
    }
}

/// Live fMP4 over WebSocket — go2rtc's `/api/ws` contract, served by the edge itself (card #138, G2).
///
/// The wire format is not ours to choose: the web player and the mobile WebView player both speak
/// go2rtc's, so this matches it exactly — the client announces the codecs it can play, we answer
/// with the MIME it should hand `addSourceBuffer`, and everything after that is fMP4 bytes.
#[cfg(feature = "gst-linked")]
async fn mse_ws(
    ws: axum::extract::ws::WebSocketUpgrade,
    Query(q): Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    let src = q.get("src").cloned().unwrap_or_default();
    // The name is pasted into an RTSP URL, so keep it to the shape the edge itself generates.
    if src.is_empty() || !src.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
        return (StatusCode::BAD_REQUEST, "bad src").into_response();
    }
    ws.on_upgrade(move |socket| mse_socket(socket, src))
}

#[cfg(feature = "gst-linked")]
async fn mse_socket(mut socket: axum::extract::ws::WebSocket, src: String) {
    use axum::extract::ws::Message;

    // Starting a pipeline waits for the first keyframe, so it must not block the async runtime.
    let name = src.clone();
    let stream = match tokio::task::spawn_blocking(move || crate::gst_mse::attach(&name)).await {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => {
            log::warn!("mse: {src} unavailable: {e}");
            return;
        }
        Err(e) => {
            log::warn!("mse: {src} attach panicked: {e}");
            return;
        }
    };
    let mut fragments = stream.subscribe();

    // The client speaks first, announcing what it can decode. We wait for it so the reply cannot
    // race ahead of the player's `sourceopen`.
    loop {
        match socket.recv().await {
            Some(Ok(Message::Text(_))) => break,
            Some(Ok(_)) => continue,
            _ => return,
        }
    }

    let hello = json!({ "type": "mse", "value": stream.mime }).to_string();
    if socket.send(Message::Text(hello)).await.is_err() {
        crate::gst_mse::detach(&src);
        return;
    }
    // `ftyp` + `moov` first — a fragment before it is undecodable.
    if socket
        .send(Message::Binary(stream.init.clone()))
        .await
        .is_err()
    {
        crate::gst_mse::detach(&src);
        return;
    }

    loop {
        tokio::select! {
            frag = fragments.recv() => match frag {
                Ok(bytes) => {
                    if socket.send(Message::Binary(bytes.to_vec())).await.is_err() {
                        break;
                    }
                }
                // A viewer that fell behind by more than the channel holds is resynced by closing:
                // the player reconnects and gets a fresh init segment.
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    log::warn!("mse: {src} viewer lagged {n} fragments — dropping it to resync");
                    break;
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            },
            // Read the socket so a client close is noticed promptly rather than at the next fragment.
            msg = socket.recv() => {
                if msg.is_none() { break; }
            }
        }
    }
    drop(fragments);
    crate::gst_mse::detach(&src);
}
