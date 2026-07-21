//! Tuya smart-home provider: list the tenant's linked devices (for the editor dropdown +
//! the Integrations page) and sync them from the Tuya Cloud. Credentials are operator-level
//! (env), never per-tenant — this only reads/caches the credential-free device catalog.
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Json;
use axum::routing::{get, post};
use axum::Router;
use serde_json::{json, Value};

use app_core::tuya::{self, TuyaConfig, TuyaDevice};

use super::{blocking, err, tenant_of, ApiErr};
use crate::auth::{self, UserClaims};
use crate::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/tuya/devices", get(list))
        .route("/tuya/sync", post(sync))
        .route("/tuya/provider", get(provider))
}

/// The tenant's linked Tuya devices (with `functions_json` so the editor can type the value widget).
async fn list(UserClaims(c): UserClaims, State(st): State<AppState>) -> Result<Json<Vec<TuyaDevice>>, ApiErr> {
    auth::require(&c, "tuya.read")?;
    let tid = tenant_of(&c)?;
    let db = st.db.clone();
    let list = blocking(move || tuya::repo::load(&db, tid)).await?;
    Ok(Json(list))
}

/// Pull the project's linked devices from the Tuya Cloud and upsert them for this tenant.
/// v1: everything associated with the operator's project lands under the caller's tenant.
async fn sync(UserClaims(c): UserClaims, State(st): State<AppState>) -> Result<Json<Value>, ApiErr> {
    auth::require(&c, "tuya.read")?;
    let tid = tenant_of(&c)?;
    let db = st.db.clone();
    let synced = blocking(move || -> Result<usize, String> {
        let cfg = TuyaConfig::from_env().ok_or("Tuya is not configured — set TUYA_ACCESS_ID / TUYA_ACCESS_KEY.")?;
        let devices = tuya::client::fetch_devices(&cfg)?;
        let n = devices.len();
        tuya::repo::upsert_many(&db, tid, &devices)?;
        Ok(n)
    })
    .await?
    .map_err(|e| err(StatusCode::BAD_GATEWAY, e))?;
    Ok(Json(json!({ "synced": synced })))
}

/// Whether the operator configured a Tuya project (env creds present) — drives the Integrations UI.
async fn provider(UserClaims(c): UserClaims, State(_st): State<AppState>) -> Result<Json<Value>, ApiErr> {
    auth::require(&c, "tuya.read")?;
    Ok(Json(json!({ "configured": TuyaConfig::from_env().is_some() })))
}
