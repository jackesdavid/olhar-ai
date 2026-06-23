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
        .route("/recordings/:id/download", get(download))
        .route("/recordings/:id", axum::routing::delete(remove))
}

#[derive(Deserialize)]
struct ListQuery {
    camera_ip: Option<String>,
    #[serde(default)]
    limit:     Option<i64>,
}

async fn list(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
    Query(q): Query<ListQuery>,
) -> Result<impl IntoResponse, ApiErr> {
    auth::require(&c, "recordings.read")?;
    let tid = tenant_of(&c)?;
    let limit = q.limit.unwrap_or(100).clamp(1, 500);
    let rows = store::list_recordings(&st.pool, tid, q.camera_ip.as_deref(), limit)
        .await
        .map_err(|e| {
            log::error!("list recordings: {e}");
            err(StatusCode::INTERNAL_SERVER_ERROR, "database error")
        })?;
    Ok(Json(rows))
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
        &st.pool, tid, &body.camera_ip, &body.clip_id,
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

// ---- edge push (called by the edge with BOX_TOKEN) --------------------------

/// The edge calls this when a recording is finalised.
#[derive(Deserialize)]
pub struct ClipPush {
    pub camera_ip:     String,
    pub clip_id:       String,
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
        clip.duration_secs,
        clip.size_bytes,
        &clip.started_at,
    )
    .await
    .map_err(|e| format!("db insert: {e}"))
}
