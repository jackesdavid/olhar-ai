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

use super::{err, tenant_of, ApiErr};
use crate::auth::{self, UserClaims};
use crate::{store, AppState};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/recordings", get(list).post(create))
        .route("/recordings/cameras", get(list_cameras))
        .route("/recordings/bulk-delete", axum::routing::post(bulk_delete))
        .route("/recordings/:id/download", get(download))
        .route("/recordings/:id", axum::routing::delete(remove))
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

    let page = store::list_recordings_filtered(
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

async fn remove(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
    Path(id): Path<i64>,
) -> Result<impl IntoResponse, ApiErr> {
    auth::require(&c, "recordings.write")?;
    let tid = tenant_of(&c)?;

    // Look up the clip_id so we can delete it from the edge disk.
    let rec = store::get_recording(&st.pool, tid, id).await.map_err(|e| {
        log::error!("get recording {id}: {e}");
        err(StatusCode::INTERNAL_SERVER_ERROR, "database error")
    })?;

    if let Some(rec) = rec {
        // Best-effort: proxy DELETE to edge. If edge returns 404 the file is already gone — still
        // clean up the DB row. Only hard-fail on a network/5xx error.
        if let Ok(Some(edge_host)) = store::box_host_for_tenant(&st.pool, tid).await {
            let base = edge_api_base(&normalize_host(&edge_host));
            let url = format!("{base}/recording/{}", urlencoding::encode(&rec.clip_id));
            match reqwest::Client::new().delete(&url).send().await {
                Ok(r) if r.status().is_server_error() => {
                    log::warn!("edge delete {url}: edge returned {}", r.status());
                    return Err(err(StatusCode::BAD_GATEWAY, "edge delete failed"));
                }
                Err(e) => {
                    log::warn!("edge delete {url}: {e}");
                    return Err(err(StatusCode::BAD_GATEWAY, format!("edge unreachable: {e}")));
                }
                _ => {} // 200 or 404 — both are fine, proceed to DB delete
            }
        }
    }

    store::delete_recording(&st.pool, tid, id).await.map_err(|e| {
        log::error!("delete recording {id}: {e}");
        err(StatusCode::INTERNAL_SERVER_ERROR, "database error")
    })?;
    Ok(StatusCode::NO_CONTENT)
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

fn normalize_host(host: &str) -> String {
    let h = host.trim().trim_end_matches('/');
    if h.starts_with("http://") || h.starts_with("https://") {
        h.to_string()
    } else {
        format!("https://{h}")
    }
}

/// The edge API (recording, snapshot) runs on :8080 via `tailscale serve --https=8080`.
/// magicdns_host is the base URL (port 443, go2rtc). Rewrite to :8080 for API calls.
fn edge_api_base(base: &str) -> String {
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

/// Bulk delete recordings by id — proxies each delete to the edge, then removes DB rows.
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

    let edge_base: Option<String> = store::box_host_for_tenant(&st.pool, tid)
        .await
        .ok()
        .flatten()
        .map(|h| edge_api_base(&normalize_host(&h)));

    let mut deleted = 0u32;
    for id in &body.ids {
        let rec = match store::get_recording(&st.pool, tid, *id).await {
            Ok(Some(r)) => r,
            _ => continue,
        };
        if let Some(ref base) = edge_base {
            let url = format!("{base}/recording/{}", urlencoding::encode(&rec.clip_id));
            let _ = reqwest::Client::new().delete(&url).send().await;
        }
        if store::delete_recording(&st.pool, tid, *id).await.is_ok() {
            deleted += 1;
        }
    }
    Ok(Json(serde_json::json!({ "deleted": deleted })))
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
