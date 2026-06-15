//! Tenant-scoped camera registry. ONVIF discovery + credential validation live on
//! the edge (LAN side); the cloud only CRUDs the device records the edge pulls.
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Json;
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
        .route("/devices/:ip/credentials", post(set_credentials))
        .route("/devices/:ip", axum::routing::delete(remove))
}

#[derive(Deserialize)]
struct CredsReq {
    username: String,
    password: String,
}

/// Stage camera credentials for the tenant's edge to collect on its next config pull.
/// **Never stored in the DB** — held in memory only until delivered, then discarded.
async fn set_credentials(UserClaims(c): UserClaims, State(st): State<AppState>, Path(ip): Path<String>, Json(b): Json<CredsReq>) -> Result<StatusCode, ApiErr> {
    auth::require(&c, "devices.write")?;
    let tid = tenant_of(&c)?;
    st.pending_creds
        .lock()
        .unwrap()
        .insert((tid, ip), (b.username, b.password, std::time::Instant::now()));
    Ok(StatusCode::NO_CONTENT)
}

/// Cameras the tenant's edge found on its LAN (ONVIF), pushed up by the edge. The admin
/// promotes one into the device list (with credentials) via PUT /devices.
async fn discovered(UserClaims(c): UserClaims, State(st): State<AppState>) -> Result<Json<Vec<store::DiscoveredRow>>, ApiErr> {
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
async fn list(UserClaims(c): UserClaims, State(st): State<AppState>) -> Result<Json<Vec<Device>>, ApiErr> {
    auth::require(&c, "devices.read")?;
    let tid = tenant_of(&c)?;
    let db = st.db.clone();
    let list = blocking(move || devices::repo::load(&db, tid)).await?;
    Ok(Json(list))
}

/// Upsert one camera by `ip` (replace in place if present, else append), persist.
async fn upsert(UserClaims(c): UserClaims, State(st): State<AppState>, Json(device): Json<Device>) -> Result<StatusCode, ApiErr> {
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
            *existing = device;
        } else {
            list.push(device);
        }
        devices::repo::save(&db, tid, &list);
    })
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Remove the camera with `ip`, persist.
async fn remove(UserClaims(c): UserClaims, State(st): State<AppState>, Path(ip): Path<String>) -> Result<StatusCode, ApiErr> {
    auth::require(&c, "devices.write")?;
    let tid = tenant_of(&c)?;
    let db = st.db.clone();
    blocking(move || {
        let mut list = devices::repo::load(&db, tid);
        list.retain(|d| d.ip != ip);
        devices::repo::save(&db, tid, &list);
    })
    .await?;
    Ok(StatusCode::NO_CONTENT)
}
