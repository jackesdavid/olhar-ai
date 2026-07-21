//! Tuya smart-home provider: list the tenant's linked devices (for the editor dropdown +
//! the Integrations page), sync them from the Tuya Cloud, and edit per-function friendly labels.
//! Credentials are operator-level (env); this reads/caches the credential-free device catalog and
//! the tenant's user-set labels. Self-service OAuth account linking is added in a follow-up.
use std::collections::BTreeMap;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Json;
use axum::routing::{get, patch, post};
use axum::Router;
use serde::Deserialize;
use serde_json::{json, Value};

use app_core::tuya::{self, TuyaConfig, TuyaDevice};

use super::{blocking, err, tenant_of, ApiErr};
use crate::auth::{self, UserClaims};
use crate::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/tuya/devices", get(list))
        .route("/tuya/devices/:device_id/labels", patch(set_labels))
        .route("/tuya/sync", post(sync))
        .route("/tuya/provider", get(provider))
}

/// The tenant's linked Tuya devices (with `functions_json` + `function_labels_json`).
async fn list(UserClaims(c): UserClaims, State(st): State<AppState>) -> Result<Json<Vec<TuyaDevice>>, ApiErr> {
    auth::require(&c, "tuya.read")?;
    let tid = tenant_of(&c)?;
    let db = st.db.clone();
    let list = blocking(move || tuya::repo::load(&db, tid)).await?;
    Ok(Json(list))
}

#[derive(Deserialize)]
struct LabelsReq {
    /// `{ "<code>": "<friendly label>" }` — the full label map for the device (replaces prior).
    labels: BTreeMap<String, String>,
}

/// Set a device's per-function friendly labels. Targeted update — survives the next sync.
async fn set_labels(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
    Path(device_id): Path<String>,
    Json(b): Json<LabelsReq>,
) -> Result<StatusCode, ApiErr> {
    auth::require(&c, "tuya.write")?;
    let tid = tenant_of(&c)?;
    // Drop empty labels so a cleared field reverts to the raw code (no dead entries).
    let map: BTreeMap<&String, &String> = b.labels.iter().filter(|(_, v)| !v.trim().is_empty()).collect();
    let labels_json = serde_json::to_string(&map).unwrap_or_else(|_| "{}".to_string());
    let db = st.db.clone();
    blocking(move || tuya::repo::set_function_labels(&db, tid, &device_id, &labels_json))
        .await?
        .map_err(|e| err(StatusCode::BAD_REQUEST, e))?;
    Ok(StatusCode::NO_CONTENT)
}

/// Pull the tenant's linked devices from the Tuya Cloud and upsert them. Scoped to the tenant's
/// OAuth-linked account uid(s); with none (operator/Default, console-linked) it takes all devices.
async fn sync(UserClaims(c): UserClaims, State(st): State<AppState>) -> Result<Json<Value>, ApiErr> {
    auth::require(&c, "tuya.read")?;
    let tid = tenant_of(&c)?;
    let db = st.db.clone();
    let synced = blocking(move || -> Result<usize, String> {
        let cfg = TuyaConfig::from_env().ok_or("Tuya is not configured — set TUYA_ACCESS_ID / TUYA_ACCESS_KEY.")?;
        let uids = tuya::repo::linked_uids(&db, tid);
        let devices = tuya::client::fetch_devices(&cfg, &uids)?;
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
