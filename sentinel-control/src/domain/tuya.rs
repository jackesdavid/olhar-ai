//! Tuya smart-home provider. Credentials are **per-tenant** ("bring your own Tuya project", like
//! Home Assistant / LocalTuya): a tenant creates their own Tuya IoT project, links their own Smart
//! Life account, and saves their Access ID / Secret here. Resolved tenant-first, falling back to
//! the operator env creds for the Default tenant. This module lists/syncs devices and edits the
//! per-function friendly labels; creds are write-only to the UI.
use std::collections::BTreeMap;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Json;
use axum::routing::{get, patch, post, put};
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
        .route("/tuya/config", put(set_config).delete(delete_config))
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
    let map: BTreeMap<&String, &String> = b.labels.iter().filter(|(_, v)| !v.trim().is_empty()).collect();
    let labels_json = serde_json::to_string(&map).unwrap_or_else(|_| "{}".to_string());
    let db = st.db.clone();
    blocking(move || tuya::repo::set_function_labels(&db, tid, &device_id, &labels_json))
        .await?
        .map_err(|e| err(StatusCode::BAD_REQUEST, e))?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct ConfigReq {
    access_id: String,
    access_key: String,
    /// Data-center base URL (e.g. https://openapi.tuyaus.com). Defaults to US when blank.
    #[serde(default)]
    endpoint: String,
}

/// Save the tenant's own Tuya project credentials. Validated (fetches a token) before saving, so a
/// wrong key / data center is caught immediately with a clear message.
async fn set_config(UserClaims(c): UserClaims, State(st): State<AppState>, Json(b): Json<ConfigReq>) -> Result<StatusCode, ApiErr> {
    auth::require(&c, "tuya.write")?;
    let tid = tenant_of(&c)?;
    let access_id = b.access_id.trim().to_string();
    let access_key = b.access_key.trim().to_string();
    let endpoint = {
        let e = b.endpoint.trim().trim_end_matches('/');
        if e.is_empty() { "https://openapi.tuyaus.com".to_string() } else { e.to_string() }
    };
    if access_id.is_empty() || access_key.is_empty() {
        return Err(err(StatusCode::BAD_REQUEST, "Access ID and Access Secret are required"));
    }
    let cfg = TuyaConfig { access_id, access_key, endpoint };
    let db = st.db.clone();
    blocking(move || -> Result<(), String> {
        tuya::client::validate(&cfg)?; // wrong keys / data center / unsubscribed API → clear error
        tuya::repo::save_config(&db, tid, &cfg)
    })
    .await?
    .map_err(|e| err(StatusCode::BAD_REQUEST, format!("Couldn't connect to Tuya: {e}")))?;
    Ok(StatusCode::NO_CONTENT)
}

/// Disconnect the tenant's Tuya account (removes their saved credentials).
async fn delete_config(UserClaims(c): UserClaims, State(st): State<AppState>) -> Result<StatusCode, ApiErr> {
    auth::require(&c, "tuya.write")?;
    let tid = tenant_of(&c)?;
    let db = st.db.clone();
    blocking(move || tuya::repo::delete_config(&db, tid)).await?.map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(StatusCode::NO_CONTENT)
}

/// Pull the tenant's linked devices from the Tuya Cloud and upsert them, using the tenant's own
/// (or the operator fallback) credentials.
async fn sync(UserClaims(c): UserClaims, State(st): State<AppState>) -> Result<Json<Value>, ApiErr> {
    auth::require(&c, "tuya.read")?;
    let tid = tenant_of(&c)?;
    let db = st.db.clone();
    let synced = blocking(move || -> Result<usize, String> {
        let cfg = tuya::repo::resolve_config(&db, tid)
            .ok_or("Tuya is not connected — add your Tuya project credentials in Settings → Integrations.")?;
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

/// Whether this tenant connected its own Tuya project — drives the UI: `none` shows the
/// "connect your account" flow, `tenant` shows the device list. Strictly per-tenant (no operator
/// fallback), so a tenant never sees another's devices.
async fn provider(UserClaims(c): UserClaims, State(st): State<AppState>) -> Result<Json<Value>, ApiErr> {
    auth::require(&c, "tuya.read")?;
    let tid = tenant_of(&c)?;
    let db = st.db.clone();
    let connected = blocking(move || tuya::repo::load_config(&db, tid).is_some()).await?;
    let source = if connected { "tenant" } else { "none" };
    Ok(Json(json!({ "configured": connected, "source": source })))
}
