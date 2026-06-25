//! Control-plane REST: auth + RBAC + master console (see `handlers`), the edge
//! camera-config pull / discovery push, the edge installer + binaries, and health.
use axum::extract::{RawQuery, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Json, Response};
use axum::routing::{get, post};
use axum::Router;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::time::Duration;
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;

use app_core::devices;
use app_core::onvif::DiscoveredDevice;
use app_core::tenancy::DEFAULT_TENANT_ID;

use crate::{handlers, store, AppState};

pub fn router(state: AppState) -> Router {
    // Edge appliance binaries (the repo is private, so they're served here, publicly,
    // instead of GitHub releases). CI ships them to EDGE_DIST_DIR on the VPS.
    let edge_dist = std::env::var("EDGE_DIST_DIR").unwrap_or_else(|_| "/edge-dist".to_string());

    Router::new()
        .route("/health", get(health))
        .route("/install.sh", get(install_script))
        .route("/edge-latest", get(edge_latest))
        .route("/config/cameras", get(cameras))
        .route("/config/commands", get(commands))
        .route("/config/discovered", post(discovered_push))
        .route("/config/device-moved", post(device_moved))
        .route("/config/streaming", post(streaming_push))
        .route("/config/logs", post(box_logs_push))
        .route("/config/motion", post(motion_push))
        .route("/config/disk-alert", post(disk_alert_push))
        .route("/config/recording", get(recording_config))
        .route("/config/recordings/clip-metadata", post(clip_push))
        .merge(handlers::router())
        .merge(crate::domain::router())
        .nest_service("/edge", ServeDir::new(edge_dist))
        .layer(CorsLayer::permissive())
        .with_state(state)
}

/// Serve the edge appliance installer so a tenant admin's one-liner works.
async fn install_script() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/x-shellscript; charset=utf-8")],
        include_str!("../../sentinel-edge/install.sh"),
    )
}

/// The latest edge build the operator has published (for the "update available" badge).
async fn edge_latest() -> impl IntoResponse {
    let version = std::env::var("LATEST_EDGE_VERSION").unwrap_or_else(|_| "0.1.0".to_string());
    Json(json!({ "version": version }))
}

async fn health(State(st): State<AppState>) -> impl IntoResponse {
    let n = devices::repo::load(&st.db, DEFAULT_TENANT_ID).len();
    Json(json!({ "status": "ok", "service": "sentinel-control", "cameras": n }))
}

/// Resolve a box token (Authorization: Bearer) → its tenant, stamping last_seen, the
/// reported tailnet host (`X-Box-Host`), and the edge version (`X-Edge-Version`).
/// `Ok(None)` = no token presented; `Err(resp)` = a token was presented but invalid.
async fn box_tenant(st: &AppState, headers: &HeaderMap) -> Result<Option<uuid::Uuid>, Response> {
    let token = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let Some(tok) = token else { return Ok(None) };

    let host = headers.get("x-box-host").and_then(|v| v.to_str().ok());
    let version = headers.get("x-edge-version").and_then(|v| v.to_str().ok());
    let (ts, ff) = box_health_headers(headers);
    let hash = hex::encode(Sha256::digest(tok.as_bytes()));
    match store::touch_box_by_token(&st.pool, &hash, host, version, ts, ff).await {
        Ok(Some(tid)) => Ok(Some(tid)),
        Ok(None) => Err((StatusCode::UNAUTHORIZED, "unknown box token").into_response()),
        Err(e) => {
            log::error!("box token lookup failed: {e}");
            Err((StatusCode::INTERNAL_SERVER_ERROR, "lookup failed").into_response())
        }
    }
}

/// Extract the edge health headers (`X-Edge-Tailscale`, `X-Edge-Ffmpeg`) — present only on
/// the box's regular config poll, absent on the high-rate pushes (then `None` keeps prior).
fn box_health_headers(headers: &HeaderMap) -> (Option<&str>, Option<bool>) {
    let ts = headers.get("x-edge-tailscale").and_then(|v| v.to_str().ok());
    let ff = headers.get("x-edge-ffmpeg").and_then(|v| v.to_str().ok()).map(|v| v.trim() == "1");
    (ts, ff)
}

/// The camera list an edge box pulls, scoped to the box's tenant (box token required —
/// this returns RTSP credentials and control is public). A dev box can opt into the
/// default tenant with `ALLOW_UNTENANTED_CONFIG=1`.
async fn cameras(State(st): State<AppState>, headers: HeaderMap, RawQuery(q): RawQuery) -> Response {
    let tenant = match box_tenant(&st, &headers).await {
        Ok(Some(tid)) => tid,
        Ok(None) => {
            let dev = std::env::var("ALLOW_UNTENANTED_CONFIG")
                .map(|v| matches!(v.trim(), "1" | "true" | "yes"))
                .unwrap_or(false);
            if dev {
                DEFAULT_TENANT_ID
            } else {
                return (StatusCode::UNAUTHORIZED, "box token required").into_response();
            }
        }
        Err(resp) => return resp,
    };

    // Long-poll: if `?wait=<secs>` and nothing is already staged, hold the request until
    // the web changes a camera (or the timeout), so edge edits apply in ~1s, not on a
    // timer (and idle edges don't re-pull/re-resolve).
    let wait: u64 = q
        .unwrap_or_default()
        .split('&')
        .find_map(|p| p.strip_prefix("wait="))
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
        .min(60);
    let has_pending = st.pending_creds.lock().unwrap().keys().any(|(t, _)| *t == tenant);
    if wait > 0 && !has_pending {
        let n = st.notifier(tenant);
        let _ = tokio::time::timeout(Duration::from_secs(wait), n.notified()).await;
    }

    let mut list = devices::repo::load(&st.db, tenant);
    // Hand over any credentials the web staged for this tenant's cameras — ONCE — then
    // discard them. Camera credentials are never persisted in the control DB.
    {
        let mut pend = st.pending_creds.lock().unwrap();
        pend.retain(|_, (_, _, t)| t.elapsed() < Duration::from_secs(1800)); // drop stale
        for d in list.iter_mut() {
            if let Some((u, p, _)) = pend.remove(&(tenant, d.ip.clone())) {
                d.username = Some(u);
                d.password = Some(p);
            }
        }
    }
    Json(list).into_response()
}

/// Like [`box_tenant`] but also resolves the box id (for log tagging).
async fn box_identity(st: &AppState, headers: &HeaderMap) -> Result<Option<(uuid::Uuid, uuid::Uuid)>, Response> {
    let token = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let Some(tok) = token else { return Ok(None) };
    let host = headers.get("x-box-host").and_then(|v| v.to_str().ok());
    let version = headers.get("x-edge-version").and_then(|v| v.to_str().ok());
    let (ts, ff) = box_health_headers(headers);
    let hash = hex::encode(Sha256::digest(tok.as_bytes()));
    match store::touch_box_identity(&st.pool, &hash, host, version, ts, ff).await {
        Ok(Some(pair)) => Ok(Some(pair)),
        Ok(None) => Err((StatusCode::UNAUTHORIZED, "unknown box token").into_response()),
        Err(e) => {
            log::error!("box token lookup failed: {e}");
            Err((StatusCode::INTERNAL_SERVER_ERROR, "lookup failed").into_response())
        }
    }
}

/// The edge polls this each cycle (box token) for pending commands; the rescan flag is
/// consumed here so the next poll won't re-trigger it.
async fn commands(State(st): State<AppState>, headers: HeaderMap) -> Response {
    let (tenant, box_id) = match box_identity(&st, &headers).await {
        Ok(Some(pair)) => pair,
        Ok(None) => return (StatusCode::UNAUTHORIZED, "box token required").into_response(),
        Err(resp) => return resp,
    };
    // rescan is tenant-wide (Devices "Rescan"); restart/update are per-box and DB-backed.
    let (restart, update) = st.take_box_commands(box_id).await;
    Json(json!({
        "rescan":  st.take_rescan(tenant),
        "restart": restart,
        "update":  update,
    })).into_response()
}

/// A camera that changed IP, reported by the edge (matched on its stable ONVIF id).
#[derive(serde::Deserialize)]
struct DeviceMove {
    onvif_id: String,
    #[serde(default)]
    old_ip: String,
    new_ip: String,
    #[serde(default)]
    xaddr: String,
}

/// The edge reports cameras whose IP changed so the cloud device rows + Live page follow
/// the move (box token required). Keyed by stable ONVIF id.
async fn device_moved(State(st): State<AppState>, headers: HeaderMap, body: Option<Json<Vec<DeviceMove>>>) -> Response {
    let tenant = match box_tenant(&st, &headers).await {
        Ok(Some(t)) => t,
        Ok(None) => return (StatusCode::UNAUTHORIZED, "box token required").into_response(),
        Err(resp) => return resp,
    };
    let moves = body.map(|Json(m)| m).unwrap_or_default();
    if moves.is_empty() {
        return StatusCode::NO_CONTENT.into_response();
    }
    let db = st.db.clone();
    let n = moves.len();
    let _ = tokio::task::spawn_blocking(move || {
        for m in &moves {
            devices::repo::move_ip(&db, tenant, &m.onvif_id, &m.old_ip, &m.new_ip, &m.xaddr);
        }
    })
    .await;
    log::info!("box reconciled {n} camera IP change(s) for tenant {tenant}");
    st.bump(tenant); // wake the edge so it re-pulls the corrected list
    StatusCode::NO_CONTENT.into_response()
}

/// The edge reports which cameras it currently has a resolved stream for (box token). Drives
/// the precise "online & correct" status dot on the Devices page.
async fn streaming_push(State(st): State<AppState>, headers: HeaderMap, body: Option<Json<Vec<String>>>) -> Response {
    let tenant = match box_tenant(&st, &headers).await {
        Ok(Some(t)) => t,
        Ok(None) => return (StatusCode::UNAUTHORIZED, "box token required").into_response(),
        Err(resp) => return resp,
    };
    let ips = body.map(|Json(v)| v).unwrap_or_default();
    if let Err(e) = store::upsert_streaming(&st.pool, tenant, &ips).await {
        log::error!("upsert_streaming failed: {e}");
        return (StatusCode::INTERNAL_SERVER_ERROR, "store failed").into_response();
    }
    StatusCode::NO_CONTENT.into_response()
}

/// One motion observation reported by the edge.
#[derive(serde::Deserialize)]
struct MotionIn {
    camera_ip: String,
    #[serde(default)]
    score: f64,
    #[serde(default)]
    area: i64,
    #[serde(default)]
    blobs: i64,
    #[serde(default)]
    bbox: [i32; 4],
    #[allow(dead_code)]
    #[serde(default)]
    at: String,
    /// The JPEG frame that triggered detection (base64). Relayed to flow so `OnEvent("motion")`
    /// can expose it as an Image output without a second round-trip to the edge.
    #[serde(default)]
    frame_b64: Option<String>,
}

/// The edge reports a motion observation (box token → tenant). We publish it onto the Redis
/// fabric (`flow:inject`) so flow fires a `motion` System event to that tenant's `OnEvent`
/// handler graphs, and (once per tenant per process) ensure the `motion` event is registered
/// with its typed payload so OnEvent nodes expose camera/score/area/blobs/bbox outputs.
async fn motion_push(State(st): State<AppState>, headers: HeaderMap, body: Option<Json<MotionIn>>) -> Response {
    let tenant = match box_tenant(&st, &headers).await {
        Ok(Some(t)) => t,
        Ok(None) => return (StatusCode::UNAUTHORIZED, "box token required").into_response(),
        Err(resp) => return resp,
    };
    let Some(Json(ev)) = body else {
        return (StatusCode::BAD_REQUEST, "motion body required").into_response();
    };

    if let Some(mut conn) = st.redis.clone() {
        // bbox is a Bbox port (canonical xyxy). The edge reports xywh, so convert here:
        // [x, y, w, h] → [x1, y1, x2, y2]. Sent as a JSON array; flow seeds it into a
        // PortValue::Bbox for the OnEvent("motion").bbox output.
        let [x, y, w, h] = ev.bbox;
        let bbox_xyxy = [x, y, x + w, y + h];
        let mut payload = serde_json::json!({
            "camera": ev.camera_ip,
            "score": ev.score.round() as i64,
            "area": ev.area,
            "blobs": ev.blobs,
            "bbox": bbox_xyxy,
        });
        if let (Some(obj), Some(f)) = (payload.as_object_mut(), ev.frame_b64) {
            obj.insert("frame".to_string(), serde_json::Value::String(f));
        }
        let msg = json!({
            "tenant": tenant.to_string(),
            "event": "motion",
            "payload": payload,
        });
        let _: Result<i64, _> = redis::cmd("PUBLISH")
            .arg("flow:inject")
            .arg(msg.to_string())
            .query_async(&mut conn)
            .await;
    } else {
        log::warn!("motion event dropped: REDIS_URL not configured");
    }
    StatusCode::NO_CONTENT.into_response()
}

/// The edge reports disk usage is near/over the configured limit (box token → tenant).
/// Publishes an `edge:full-disk` system event to the Redis flow:inject fabric so tenant
/// `OnEvent("edge:full-disk")` workflow graphs can react (send alert, stop recordings, etc.).
async fn disk_alert_push(State(st): State<AppState>, headers: HeaderMap, body: Option<Json<serde_json::Value>>) -> Response {
    let tenant = match box_tenant(&st, &headers).await {
        Ok(Some(t)) => t,
        Ok(None) => return (StatusCode::UNAUTHORIZED, "box token required").into_response(),
        Err(resp) => return resp,
    };
    let payload = body.map(|Json(v)| v).unwrap_or_default();
    if let Some(mut conn) = st.redis.clone() {
        let msg = json!({
            "tenant": tenant.to_string(),
            "event":  "edge:full-disk",
            "payload": payload,
        });
        let _: Result<i64, _> = redis::cmd("PUBLISH")
            .arg("flow:inject")
            .arg(msg.to_string())
            .query_async(&mut conn)
            .await;
    } else {
        log::warn!("edge:full-disk event dropped: REDIS_URL not configured");
    }
    StatusCode::NO_CONTENT.into_response()
}

/// Newest box-log rows kept per box (bounded retention; the edge keeps its own ring + stdout).
const BOX_LOG_KEEP: i64 = 5000;

/// The edge pushes a batch of its recent log lines here (box token). Control persists a
/// bounded tail and re-broadcasts each line for any live per-box log viewer.
async fn box_logs_push(State(st): State<AppState>, headers: HeaderMap, body: Option<Json<Vec<store::LogLineIn>>>) -> Response {
    let (tenant, box_id) = match box_identity(&st, &headers).await {
        Ok(Some(pair)) => pair,
        Ok(None) => return (StatusCode::UNAUTHORIZED, "box token required").into_response(),
        Err(resp) => return resp,
    };
    let lines = body.map(|Json(l)| l).unwrap_or_default();
    if lines.is_empty() {
        return StatusCode::NO_CONTENT.into_response();
    }
    // Live fan-out (tagged with tenant+box so the per-box SSE relay can filter).
    // Also push to the unified platform_logs queue (fire-and-forget; edge logs are module=edge).
    for l in &lines {
        let msg = json!({ "tenant": tenant, "box": box_id, "ts": l.ts, "level": l.level, "target": l.target, "message": l.message });
        let _ = st.log_bus.send(msg.to_string());
        st.log_queue.push(app_core::platform_logs::PlatformLog {
            tenant_id: tenant,
            module: "edge",
            trace_id: None,
            entity_id: Some(box_id.to_string()),
            entity_type: Some("edge_box"),
            level: match l.level.to_lowercase().as_str() {
                "error" => "error",
                "warn"  => "warn",
                "debug" => "debug",
                _       => "info",
            },
            message: l.message.clone(),
            payload: Some(json!({ "target": l.target, "ts": l.ts })),
        });
    }
    if let Err(e) = store::insert_box_logs(&st.pool, tenant, box_id, &lines, BOX_LOG_KEEP).await {
        log::error!("insert_box_logs failed: {e}");
    }
    StatusCode::NO_CONTENT.into_response()
}

/// The edge pushes its latest ONVIF discovery sweep here (box token required); control
/// stores it tenant-scoped for the cloud Devices page.
async fn discovered_push(State(st): State<AppState>, headers: HeaderMap, body: Option<Json<Vec<DiscoveredDevice>>>) -> Response {
    let tenant = match box_tenant(&st, &headers).await {
        Ok(Some(tid)) => tid,
        Ok(None) => return (StatusCode::UNAUTHORIZED, "box token required").into_response(),
        Err(resp) => return resp,
    };
    let cams = body.map(|Json(c)| c).unwrap_or_default();
    match store::upsert_discovered(&st.pool, tenant, &cams).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => {
            log::error!("upsert_discovered failed: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, "store failed").into_response()
        }
    }
}

/// Return the recording config for the authenticated edge box.
async fn recording_config(State(st): State<AppState>, headers: HeaderMap) -> Response {
    let token = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let Some(tok) = token else {
        return (StatusCode::UNAUTHORIZED, "box token required").into_response();
    };
    let hash = hex::encode(Sha256::digest(tok.as_bytes()));
    match store::recording_config_by_token(&st.pool, &hash).await {
        Ok(Some(cfg)) => Json(cfg).into_response(),
        Ok(None) => (StatusCode::UNAUTHORIZED, "unknown box token").into_response(),
        Err(e) => {
            log::error!("recording_config_by_token: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, "db error").into_response()
        }
    }
}

/// The edge pushes a finalised clip manifest (box token). Persists metadata
/// and fans out a `recording-ready` SSE event to the tenant's browsers.
async fn clip_push(State(st): State<AppState>, headers: HeaderMap, body: Option<Json<crate::domain::recordings::ClipPush>>) -> Response {
    let tenant = match box_tenant(&st, &headers).await {
        Ok(Some(t)) => t,
        Ok(None) => return (StatusCode::UNAUTHORIZED, "box token required").into_response(),
        Err(resp) => return resp,
    };
    let Some(Json(clip)) = body else {
        return (StatusCode::BAD_REQUEST, "clip body required").into_response();
    };
    match crate::domain::recordings::push_clip(&st.pool, tenant, clip).await {
        Ok(id) => Json(serde_json::json!({ "id": id })).into_response(),
        Err(e) => {
            log::error!("clip push failed: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, e).into_response()
        }
    }
}
