//! Tenant-scoped camera registry. ONVIF discovery + credential validation live on
//! the edge (LAN side); the cloud only CRUDs the device records the edge pulls.
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Json;
use axum::routing::get;
use axum::Router;

use app_core::devices::{self, Device};

use super::{blocking, tenant_of, ApiErr};
use crate::auth::{self, UserClaims};
use crate::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/devices", get(list).put(upsert)).route("/devices/:ip", axum::routing::delete(remove))
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
