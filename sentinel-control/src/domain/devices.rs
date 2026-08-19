//! Tenant-scoped camera registry. ONVIF discovery + credential validation live on
//! the edge (LAN side); the cloud only CRUDs the device records the edge pulls.
use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Json, Response};
use axum::routing::{get, post};
use axum::Router;
use serde::Deserialize;

use app_core::devices::{self, Device};

use super::{blocking, err, tenant_of, ApiErr};
use crate::auth::{self, UserClaims};
use crate::{store, AppState};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/devices", get(list).put(upsert))
        .route("/devices/discovered", get(discovered))
        .route("/devices/configured", get(configured))
        .route("/devices/streaming", get(streaming))
        .route("/devices/:ip/credentials", post(set_credentials))
        .route("/devices/:ip/probe", post(probe))
        .route("/devices/:ip/snapshot", get(snapshot))
        .route("/devices/:ip/tracks", get(tracks))
        .route("/devices/rescan", post(rescan))
        .route("/devices/:ip", axum::routing::delete(remove))
}

/// Ask the tenant's edge box(es) to re-run ONVIF discovery now (e.g. after a camera got a
/// new DHCP IP). Queues a command the edge consumes on its next poll (woken immediately);
/// moved cameras are reconciled by stable ONVIF id on the edge + pushed back.
async fn rescan(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
) -> Result<StatusCode, ApiErr> {
    auth::require(&c, "devices.write")?;
    let tid = tenant_of(&c)?;
    st.request_rescan(tid);
    Ok(StatusCode::NO_CONTENT)
}

/// IPs of cameras whose credentials have been set (relayed to the edge). Lets the UI
/// show "Configured" without the cloud ever holding the credentials.
async fn configured(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
) -> Result<Json<Vec<String>>, ApiErr> {
    auth::require(&c, "devices.read")?;
    let tid = tenant_of(&c)?;
    store::list_configured(&st.pool, tid)
        .await
        .map(Json)
        .map_err(|e| {
            log::error!("list_configured failed: {e}");
            err(StatusCode::INTERNAL_SERVER_ERROR, "database error")
        })
}

/// One camera the tenant's edge has a resolved stream for, plus the codec it probed off the
/// camera's own SDP ("H264"/"H265"), when known.
#[derive(serde::Serialize)]
struct StreamingCam {
    ip: String,
    codec: Option<String>,
}

/// Cameras the tenant's edge currently has a resolved stream for — drives the "online" status
/// dot (creds valid + camera reachable) and the codec badge. The cloud never holds the RTSP URL
/// itself.
async fn streaming(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
) -> Result<Json<Vec<StreamingCam>>, ApiErr> {
    auth::require(&c, "devices.read")?;
    let tid = tenant_of(&c)?;
    store::list_streaming(&st.pool, tid)
        .await
        .map(|rows| {
            Json(
                rows.into_iter()
                    .map(|(ip, codec)| StreamingCam { ip, codec })
                    .collect(),
            )
        })
        .map_err(|e| {
            log::error!("list_streaming failed: {e}");
            err(StatusCode::INTERNAL_SERVER_ERROR, "database error")
        })
}

#[derive(Deserialize)]
struct CredsReq {
    username: String,
    password: String,
    /// Explicit RTSP URL for a source that does not speak ONVIF (#175) — a phone running an
    /// RTSP app, or a camera whose ONVIF is broken. Absent for the normal discovered path, where
    /// the edge resolves the URL itself.
    #[serde(default)]
    rtsp_url: Option<String>,
}

/// Stage camera credentials for the tenant's edge to collect on its next config pull.
/// **Never stored in the DB** — held in memory only until delivered, then discarded.
async fn set_credentials(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
    Path(ip): Path<String>,
    Json(b): Json<CredsReq>,
) -> Result<StatusCode, ApiErr> {
    auth::require(&c, "devices.write")?;
    let tid = tenant_of(&c)?;
    let rtsp_url = b
        .rtsp_url
        .map(|u| u.trim().to_string())
        .filter(|u| !u.is_empty());
    st.pending_creds.lock().unwrap().insert(
        (tid, ip.clone()),
        (b.username, b.password, rtsp_url, std::time::Instant::now()),
    );
    // Mark configured (non-sensitive flag for the UI) + wake the edge's long-poll now.
    let _ = store::mark_configured(&st.pool, tid, &ip).await;
    st.bump(tid);
    Ok(StatusCode::NO_CONTENT)
}

/// Ask the tenant's edge to prove a camera's credentials, **without saving them anywhere**
/// (card #88). The creds ride the request to the edge and are dropped after the probe — they
/// are neither staged in `pending_creds` nor written to the DB.
///
/// This is the "test" half of test-then-save: the UI calls it first and only calls
/// `set_credentials` on a verified `ok`. The edge returns a typed verdict (`ok`, `auth_failed`,
/// `locked`, `not_found`, `unreachable`, `timeout`, `error`) so the UI can name the real cause
/// instead of blaming the password for a locked or unreachable camera.
async fn probe(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
    Path(ip): Path<String>,
    Json(b): Json<CredsReq>,
) -> Result<Response, ApiErr> {
    // Read-only for our data (nothing is persisted), but it does drive a camera login, so it
    // sits behind the same permission as setting credentials.
    auth::require(&c, "devices.write")?;
    let tid = tenant_of(&c)?;
    let path = format!("/probe/{}", urlencoding::encode(&ip));
    let creds = serde_json::json!({ "username": b.username, "password": b.password });

    // Prefer the SSE command channel (card #144) — it works for an outbound-only box with no
    // reachable inbound, which is exactly why the Devices "Test" used to fail on those. Fall back
    // to the legacy direct POST for a box that isn't on SSE.
    let (status, body): (u16, Vec<u8>) = if let Some(res) =
        app_core::edge_commands::edge_http_via_sse(
            &st.pool,
            tid,
            "POST",
            &path,
            Some(creds.clone()),
            35,
        )
        .await
    {
        res.map_err(|e| err(StatusCode::BAD_GATEWAY, format!("edge probe over SSE: {e}")))?
    } else {
        use crate::domain::recordings::{edge_api_base, normalize_host};
        let edge_host = store::box_host_for_tenant(&st.pool, tid)
            .await
            .map_err(|e| {
                log::error!("box host lookup: {e}");
                err(StatusCode::INTERNAL_SERVER_ERROR, "database error")
            })?
            .ok_or_else(|| {
                err(
                    StatusCode::BAD_GATEWAY,
                    "no edge box connected for this tenant",
                )
            })?;
        let base = edge_api_base(&normalize_host(&edge_host));
        let url = format!("{base}{path}");
        // The probe opens a real socket to the camera, so allow for a slow handshake — but bound it.
        let resp = st
            .http
            .post(&url)
            .json(&creds)
            .timeout(std::time::Duration::from_secs(30))
            .send()
            .await
            .map_err(|e| {
                log::error!("edge probe {url}: {e}");
                err(StatusCode::BAD_GATEWAY, format!("edge unreachable: {e}"))
            })?;
        let status = resp.status().as_u16();
        let body = resp
            .bytes()
            .await
            .map_err(|e| err(StatusCode::BAD_GATEWAY, format!("edge response read: {e}")))?
            .to_vec();
        (status, body)
    };

    if !(200..300).contains(&status) {
        return Err(err(
            StatusCode::BAD_GATEWAY,
            format!("edge returned {status}"),
        ));
    }
    Ok((
        [(header::CONTENT_TYPE, "application/json")],
        Body::from(body),
    )
        .into_response())
}

/// Cameras the tenant's edge found on its LAN (ONVIF), pushed up by the edge. The admin
/// promotes one into the device list (with credentials) via PUT /devices.
async fn discovered(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
) -> Result<Json<Vec<store::DiscoveredRow>>, ApiErr> {
    auth::require(&c, "devices.read")?;
    let tid = tenant_of(&c)?;
    store::list_discovered(&st.pool, tid)
        .await
        .map(Json)
        .map_err(|e| {
            log::error!("list_discovered failed: {e}");
            err(StatusCode::INTERNAL_SERVER_ERROR, "database error")
        })
}

/// All cameras for the caller's tenant.
async fn list(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
) -> Result<Json<Vec<Device>>, ApiErr> {
    auth::require(&c, "devices.read")?;
    let tid = tenant_of(&c)?;
    let db = st.db.clone();
    let list = blocking(move || devices::repo::load(&db, tid)).await?;
    Ok(Json(list))
}

/// Upsert one camera by `ip` (replace in place if present, else append), persist.
async fn upsert(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
    Json(device): Json<Device>,
) -> Result<StatusCode, ApiErr> {
    auth::require(&c, "devices.write")?;
    let tid = tenant_of(&c)?;
    // Camera credentials are NEVER persisted in the cloud — they go to the edge via
    // POST /devices/:ip/credentials. Strip any that slipped into the device payload.
    let mut device = device;
    device.username = None;
    device.password = None;
    device.rtsp_url = None;
    let db = st.db.clone();
    blocking(move || {
        let mut list = devices::repo::load(&db, tid);
        if let Some(existing) = list.iter_mut().find(|d| d.ip == device.ip) {
            *existing = device; // editing an existing camera never hits the plan cap
        } else {
            // Adding one: the tenant's plan caps how many cameras it may run.
            let cap = app_core::billing::subscriptions::camera_cap(&db, tid);
            if list.len() as i32 >= cap {
                return Err(format!(
                    "Your plan allows {cap} camera{} — upgrade to add more.",
                    if cap == 1 { "" } else { "s" }
                ));
            }
            list.push(device);
        }
        devices::repo::save(&db, tid, &list);
        Ok::<_, String>(())
    })
    .await?
    .map_err(|e| err(StatusCode::PAYMENT_REQUIRED, e))?;
    st.bump(tid); // wake the edge's long-poll
    Ok(StatusCode::NO_CONTENT)
}

/// Proxy a live JPEG snapshot of camera `ip` from the tenant's edge box (`GET
/// /snapshot/:ip` on the edge API) — used by the workflow editor's line-drawing UI (card:
/// edge tracking) to show a frame to trace a crossing line over. Mirrors the recording
/// `download` proxy's box-resolution + passthrough shape.
async fn snapshot(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
    Path(ip): Path<String>,
) -> Result<Response, ApiErr> {
    use crate::domain::recordings::{edge_api_base, normalize_host};
    auth::require(&c, "devices.read")?;
    let tid = tenant_of(&c)?;

    // SSE command channel first (card #144 — outbound-only boxes); direct fallback below.
    let path = format!("/snapshot/{}", urlencoding::encode(&ip));
    if let Some(res) =
        app_core::edge_commands::edge_http_via_sse(&st.pool, tid, "GET", &path, None, 20).await
    {
        let (status, body) = res.map_err(|e| {
            err(
                StatusCode::BAD_GATEWAY,
                format!("edge snapshot over SSE: {e}"),
            )
        })?;
        if status == 404 {
            return Err(err(StatusCode::NOT_FOUND, "no frame available"));
        }
        if !(200..300).contains(&status) {
            return Err(err(
                StatusCode::BAD_GATEWAY,
                format!("edge returned {status}"),
            ));
        }
        return Ok((
            [
                (header::CONTENT_TYPE, "image/jpeg"),
                (header::CACHE_CONTROL, "no-store"),
            ],
            Body::from(body),
        )
            .into_response());
    }

    let edge_host = store::box_host_for_tenant(&st.pool, tid)
        .await
        .map_err(|e| {
            log::error!("box host lookup: {e}");
            err(StatusCode::INTERNAL_SERVER_ERROR, "database error")
        })?
        .ok_or_else(|| {
            err(
                StatusCode::BAD_GATEWAY,
                "no edge box connected for this tenant",
            )
        })?;

    let base = edge_api_base(&normalize_host(&edge_host));
    let url = format!("{base}/snapshot/{}", urlencoding::encode(&ip));

    let resp = reqwest::get(&url).await.map_err(|e| {
        log::error!("edge snapshot fetch {url}: {e}");
        err(StatusCode::BAD_GATEWAY, format!("edge unreachable: {e}"))
    })?;

    if !resp.status().is_success() {
        let edge_status = resp.status();
        let our_status = if edge_status == reqwest::StatusCode::NOT_FOUND {
            StatusCode::NOT_FOUND
        } else {
            StatusCode::BAD_GATEWAY
        };
        return Err(err(our_status, format!("edge returned {edge_status}")));
    }

    let stream = resp.bytes_stream();
    Ok((
        [
            (header::CONTENT_TYPE, "image/jpeg"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        Body::from_stream(stream),
    )
        .into_response())
}

/// Proxy the tracker's live "state of the world" for camera `ip` from the tenant's edge box
/// (`GET /tracks/:ip` on the edge API) — the `GetTrackedObjects` node's backing call. JSON
/// passthrough (unlike `snapshot`'s binary), otherwise the same box-resolution shape.
async fn tracks(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
    Path(ip): Path<String>,
) -> Result<Response, ApiErr> {
    use crate::domain::recordings::{edge_api_base, normalize_host};
    auth::require(&c, "devices.read")?;
    let tid = tenant_of(&c)?;

    // SSE command channel first (card #144); direct fallback below.
    let path = format!("/tracks/{}", urlencoding::encode(&ip));
    if let Some(res) =
        app_core::edge_commands::edge_http_via_sse(&st.pool, tid, "GET", &path, None, 15).await
    {
        let (status, body) = res.map_err(|e| {
            err(
                StatusCode::BAD_GATEWAY,
                format!("edge tracks over SSE: {e}"),
            )
        })?;
        if !(200..300).contains(&status) {
            return Err(err(
                StatusCode::BAD_GATEWAY,
                format!("edge returned {status}"),
            ));
        }
        return Ok((
            [(header::CONTENT_TYPE, "application/json")],
            Body::from(body),
        )
            .into_response());
    }

    let edge_host = store::box_host_for_tenant(&st.pool, tid)
        .await
        .map_err(|e| {
            log::error!("box host lookup: {e}");
            err(StatusCode::INTERNAL_SERVER_ERROR, "database error")
        })?
        .ok_or_else(|| {
            err(
                StatusCode::BAD_GATEWAY,
                "no edge box connected for this tenant",
            )
        })?;

    let base = edge_api_base(&normalize_host(&edge_host));
    let url = format!("{base}/tracks/{}", urlencoding::encode(&ip));

    let resp = reqwest::get(&url).await.map_err(|e| {
        log::error!("edge tracks fetch {url}: {e}");
        err(StatusCode::BAD_GATEWAY, format!("edge unreachable: {e}"))
    })?;

    if !resp.status().is_success() {
        return Err(err(
            StatusCode::BAD_GATEWAY,
            format!("edge returned {}", resp.status()),
        ));
    }

    let body = resp
        .bytes()
        .await
        .map_err(|e| err(StatusCode::BAD_GATEWAY, format!("edge response read: {e}")))?;
    Ok((
        [(header::CONTENT_TYPE, "application/json")],
        Body::from(body),
    )
        .into_response())
}

/// Remove the camera with `ip`, persist.
async fn remove(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
    Path(ip): Path<String>,
) -> Result<StatusCode, ApiErr> {
    auth::require(&c, "devices.write")?;
    let tid = tenant_of(&c)?;
    let ipc = ip.clone();
    let db = st.db.clone();
    blocking(move || {
        let mut list = devices::repo::load(&db, tid);
        list.retain(|d| d.ip != ipc);
        devices::repo::save(&db, tid, &list);
    })
    .await?;
    let _ = store::unmark_configured(&st.pool, tid, &ip).await;
    st.bump(tid); // wake the edge's long-poll
    Ok(StatusCode::NO_CONTENT)
}
