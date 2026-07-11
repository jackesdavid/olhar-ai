//! Recordings — metadata index only. Video files never leave the edge.
//!
//! Flow:
//!   1. Edge finishes a clip → pushes metadata (camera_ip, clip_id, duration, size,
//!      started_at) to `POST /config/recordings/clip`. Control persists only those fields.
//!   2. Browser lists clips via `GET /recordings` — gets metadata rows, no video bytes.
//!   3. Browser downloads a clip via `GET /recordings/:id/download` — control resolves the
//!      tenant's edge MagicDNS host and proxies the file bytes from the edge on-demand.
//!      Nothing is buffered or stored cloud-side; the proxy is a pass-through stream.
//!
//! The edge is the sole source of truth for the actual .mp4 files.
use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Json, Response};
use axum::routing::get;
use axum::Router;
use serde::Deserialize;

use app_core::work_queue;

use super::{err, tenant_of, ApiErr};
use crate::auth::{self, UserClaims};
use crate::{store, AppState};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/recordings", get(list).post(create))
        .route("/recordings/cameras", get(list_cameras))
        .route("/recordings/bulk-delete", axum::routing::post(bulk_delete))
        .route("/recordings/:id/url", get(cloud_url))
        .route("/recordings/:id/download", get(download))
        .route("/recordings/:id", axum::routing::delete(remove))
}

/// How long a playback URL stays valid. Long enough to watch a clip start-to-finish and seek
/// around; short enough that a leaked URL expires before it's useful.
const PLAYBACK_URL_TTL_SECS: u32 = 3600;

/// A short-lived URL to stream a **cloud** clip straight from R2 — no bytes cross this VPS, and
/// the clip survives the edge being offline (the whole point of the durable copy). Returns 409
/// for a clip that isn't in the cloud: the caller should fall back to the edge download proxy.
async fn cloud_url(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
    Path(id): Path<i64>,
) -> Result<impl IntoResponse, ApiErr> {
    auth::require(&c, "recordings.read")?;
    let tid = tenant_of(&c)?;

    let key = store::recording_cloud_key(&st.pool, tid, id)
        .await
        .map_err(|e| {
            log::error!("recording cloud key {id}: {e}");
            err(StatusCode::INTERNAL_SERVER_ERROR, "database error")
        })?
        .ok_or_else(|| err(StatusCode::NOT_FOUND, "recording not found"))?
        .ok_or_else(|| err(StatusCode::CONFLICT, "clip is not in the cloud"))?;

    // `presign_get` builds its own runtime — it must not run on the async worker.
    let url = tokio::task::spawn_blocking(move || app_core::clip_store::presign_get(&key, PLAYBACK_URL_TTL_SECS))
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, format!("task: {e}")))?
        .map_err(|e| {
            log::error!("presign get {id}: {e}");
            err(StatusCode::BAD_GATEWAY, "could not sign playback url")
        })?;

    Ok(Json(serde_json::json!({ "url": url })))
}

#[derive(Deserialize)]
struct ListQuery {
    camera_ip: Option<String>,
    from:      Option<String>,
    to:        Option<String>,
    sort:      Option<String>,
    dir:       Option<String>,
    #[serde(default)]
    limit:     Option<i64>,
    #[serde(default)]
    offset:    Option<i64>,
}

async fn list(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
    Query(q): Query<ListQuery>,
) -> Result<impl IntoResponse, ApiErr> {
    auth::require(&c, "recordings.read")?;
    let tid = tenant_of(&c)?;
    let limit  = q.limit.unwrap_or(50).clamp(1, 500);
    let offset = q.offset.unwrap_or(0).max(0);
    let sort   = q.sort.as_deref().unwrap_or("started_at");
    let dir    = q.dir.as_deref().unwrap_or("desc");

    let mut page = store::list_recordings_filtered(
        &st.pool, tid,
        q.camera_ip.as_deref(),
        q.from.as_deref(),
        q.to.as_deref(),
        sort, dir, limit, offset,
    )
    .await
    .map_err(|e| {
        log::error!("list recordings: {e}");
        err(StatusCode::INTERNAL_SERVER_ERROR, "database error")
    })?;

    // Mark clips with an in-flight delete so the UI can show them as "deleting". One extra query,
    // scoped to this tenant's open jobs — the row is still present until the db job drops it.
    if let Ok(open) = work_queue::open_recording_ids(&st.pool, tid).await {
        if !open.is_empty() {
            let open: std::collections::HashSet<i64> = open.into_iter().collect();
            for item in &mut page.items {
                item.deleting = open.contains(&item.id);
            }
        }
    }

    Ok(Json(serde_json::json!({ "total": page.total, "items": page.items })))
}

/// Create a recording row directly (JWT auth, recordings.write). Used for manual imports
/// and E2E test seeding — the normal path is the edge pushing via /config/recordings/clip.
async fn create(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
    axum::Json(body): axum::Json<ClipPush>,
) -> Result<impl IntoResponse, ApiErr> {
    auth::require(&c, "recordings.write")?;
    let tid = tenant_of(&c)?;
    let id = store::insert_recording(
        &st.pool, tid, &body.camera_ip, &body.clip_id, &body.friendly_name,
        body.duration_secs, body.size_bytes, &body.started_at,
    )
    .await
    .map_err(|e| {
        log::error!("create recording: {e}");
        err(StatusCode::INTERNAL_SERVER_ERROR, "database error")
    })?;
    Ok((StatusCode::CREATED, axum::Json(serde_json::json!({ "id": id }))))
}

/// The work-queue category for clip deletion, and its three actions. One delete request enqueues
/// one job of each kind under a shared `request_id`; a per-kind consumer drains them idempotently.
pub const DELETE_CATEGORY: &str = "clip_delete";
pub const KIND_EDGE: &str = "edge";
pub const KIND_CLOUD: &str = "cloud";
pub const KIND_DB: &str = "db";

/// Enqueue the three deletion jobs for one clip. Each carries its own target in the payload, so
/// the queue — not the recording row — is the durable pointer: even once the `db` job drops the
/// row, the `cloud` job still holds the key and finishes. Returns whether the clip existed.
async fn enqueue_delete(st: &AppState, tenant: uuid::Uuid, id: i64) -> Result<bool, ApiErr> {
    let Some((clip_id, cloud_key)) = store::recording_for_delete(&st.pool, tenant, id).await.map_err(|e| {
        log::error!("recording_for_delete {id}: {e}");
        err(StatusCode::INTERNAL_SERVER_ERROR, "database error")
    })?
    else {
        return Ok(false);
    };
    let payload = serde_json::json!({
        "recording_id": id.to_string(),  // text so the JSONB index / open-check matches cleanly
        "clip_id": clip_id,
        "cloud_key": cloud_key,
    });
    let jobs = vec![
        work_queue::NewJob { kind: KIND_EDGE.into(),  payload: payload.clone() },
        work_queue::NewJob { kind: KIND_CLOUD.into(), payload: payload.clone() },
        work_queue::NewJob { kind: KIND_DB.into(),    payload },
    ];
    let request_id = uuid::Uuid::new_v4();
    work_queue::enqueue(&st.pool, request_id, tenant, DELETE_CATEGORY, &jobs).await.map_err(|e| {
        log::error!("enqueue delete {id}: {e}");
        err(StatusCode::INTERNAL_SERVER_ERROR, "could not queue deletion")
    })?;
    Ok(true)
}

/// Audit a customer-initiated deletion — the durable record a data-subject request is answered
/// with. Written at the moment of intent, so it survives even if the jobs finish much later.
fn audit_delete(st: &AppState, tenant: uuid::Uuid, actor: &str, count: u64) {
    st.log_queue.push(app_core::platform_logs::PlatformLog {
        tenant_id: tenant,
        module: "recordings",
        trace_id: None,
        entity_id: None,
        entity_type: Some("recording"),
        level: "info",
        message: format!("customer requested deletion of {count} clip(s)"),
        payload: Some(serde_json::json!({ "count": count, "by": actor })),
    });
}

async fn remove(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
    Path(id): Path<i64>,
) -> Result<impl IntoResponse, ApiErr> {
    auth::require(&c, "recordings.write")?;
    let tid = tenant_of(&c)?;

    // Enqueue the deletion and return — the clip shows as "deleting" (a job is open for it) until
    // the jobs finish edge + cloud, then the db job drops the row. Durable across a restart.
    if !enqueue_delete(&st, tid, id).await? {
        return Err(err(StatusCode::NOT_FOUND, "recording not found"));
    }
    audit_delete(&st, tid, &c.sub, 1);
    Ok(StatusCode::ACCEPTED)
}

/// Stream a clip from the edge over the tailnet, proxying through control so
/// the browser doesn't need to be on the Tailscale network.
/// Stream a clip from the edge to the browser. Control resolves the tenant's edge
/// MagicDNS host and proxies bytes on-demand — no video is stored cloud-side.
async fn download(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Response, ApiErr> {
    auth::require(&c, "recordings.read")?;
    let tid = tenant_of(&c)?;

    let rec = store::get_recording(&st.pool, tid, id)
        .await
        .map_err(|e| {
            log::error!("get recording {id}: {e}");
            err(StatusCode::INTERNAL_SERVER_ERROR, "database error")
        })?
        .ok_or_else(|| err(StatusCode::NOT_FOUND, "recording not found"))?;

    let edge_host = store::box_host_for_tenant(&st.pool, tid)
        .await
        .map_err(|e| {
            log::error!("box host lookup: {e}");
            err(StatusCode::INTERNAL_SERVER_ERROR, "database error")
        })?
        .ok_or_else(|| err(StatusCode::BAD_GATEWAY, "no edge box connected for this tenant"))?;

    // magicdns_host is the go2rtc URL (port 443). The edge API runs on :8080.
    let base = normalize_host(&edge_host);
    let base = edge_api_base(&base);
    let url = format!("{base}/recording/{}", urlencoding::encode(&rec.clip_id));

    let resp = reqwest::get(&url).await.map_err(|e| {
        log::error!("edge clip fetch {url}: {e}");
        err(StatusCode::BAD_GATEWAY, format!("edge unreachable: {e}"))
    })?;

    if !resp.status().is_success() {
        let edge_status = resp.status();
        // Pass 404 through so the browser shows "not found" rather than a confusing gateway error.
        let our_status = if edge_status == reqwest::StatusCode::NOT_FOUND {
            StatusCode::NOT_FOUND
        } else {
            StatusCode::BAD_GATEWAY
        };
        return Err(err(our_status, format!("edge returned {edge_status}")));
    }

    let filename = format!("{}.mp4", rec.clip_id);
    let stream = resp.bytes_stream();
    Ok((
        [
            (header::CONTENT_TYPE, "video/mp4".to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{filename}\""),
            ),
        ],
        Body::from_stream(stream),
    )
        .into_response())
}

pub fn normalize_host(host: &str) -> String {
    let h = host.trim().trim_end_matches('/');
    if h.starts_with("http://") || h.starts_with("https://") {
        h.to_string()
    } else {
        format!("https://{h}")
    }
}

/// The edge API (recording, snapshot) runs on :8080 via `tailscale serve --https=8080`.
/// magicdns_host is the base URL (port 443, go2rtc). Rewrite to :8080 for API calls.
pub fn edge_api_base(base: &str) -> String {
    // If it already has a port, use as-is.
    let after_scheme = base.trim_start_matches("https://").trim_start_matches("http://");
    if after_scheme.contains(':') {
        return base.to_string();
    }
    format!("{base}:8080")
}

/// Discovered cameras for this tenant — used to populate the camera filter dropdown.
async fn list_cameras(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
) -> Result<impl IntoResponse, ApiErr> {
    auth::require(&c, "recordings.read")?;
    let tid = tenant_of(&c)?;
    let cameras = store::list_discovered(&st.pool, tid).await.map_err(|e| {
        log::error!("list cameras for recordings: {e}");
        err(StatusCode::INTERNAL_SERVER_ERROR, "database error")
    })?;
    let result: Vec<_> = cameras
        .into_iter()
        .map(|cam| serde_json::json!({ "ip": cam.ip, "name": cam.name }))
        .collect();
    Ok(Json(result))
}

/// Bulk delete recordings by id — enqueues the three deletion jobs for each clip.
#[derive(Deserialize)]
struct BulkDeleteBody {
    ids: Vec<i64>,
}

async fn bulk_delete(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
    axum::Json(body): axum::Json<BulkDeleteBody>,
) -> Result<impl IntoResponse, ApiErr> {
    auth::require(&c, "recordings.write")?;
    let tid = tenant_of(&c)?;

    let mut queued = 0u64;
    for id in &body.ids {
        if enqueue_delete(&st, tid, *id).await? {
            queued += 1;
        }
    }
    if queued > 0 {
        audit_delete(&st, tid, &c.sub, queued);
    }
    Ok(Json(serde_json::json!({ "deleted": queued })))
}

// ---- edge push (called by the edge with BOX_TOKEN) --------------------------

/// The edge calls this when a recording is finalised.
#[derive(Deserialize)]
pub struct ClipPush {
    pub camera_ip:     String,
    pub clip_id:       String,
    #[serde(default)]
    pub friendly_name: String,
    pub duration_secs: i32,
    pub size_bytes:    i64,
    pub started_at:    String,
}

/// Accept a finalised clip from the edge. Authenticated via the same BOX_TOKEN
/// flow as `/config/cameras` — the handler is registered in http.rs.
pub async fn push_clip(
    pool: &sqlx::PgPool,
    tenant_id: uuid::Uuid,
    clip: ClipPush,
) -> Result<i64, String> {
    store::insert_recording(
        pool,
        tenant_id,
        &clip.camera_ip,
        &clip.clip_id,
        &clip.friendly_name,
        clip.duration_secs,
        clip.size_bytes,
        &clip.started_at,
    )
    .await
    .map_err(|e| format!("db insert: {e}"))
}
