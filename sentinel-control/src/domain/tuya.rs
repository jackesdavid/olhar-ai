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
        .route("/tuya/console", put(set_console))
        .route("/tuya/config", put(set_config).delete(delete_config))
        .route("/tuya/diagnose", post(diagnose))
        .route("/tuya/devices/:device_id/test", post(test_device))
}

/// The tenant's linked Tuya devices (with `functions_json` + `function_labels_json`).
async fn list(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
) -> Result<Json<Vec<TuyaDevice>>, ApiErr> {
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
    let map: BTreeMap<&String, &String> = b
        .labels
        .iter()
        .filter(|(_, v)| !v.trim().is_empty())
        .collect();
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
async fn set_config(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
    Json(b): Json<ConfigReq>,
) -> Result<StatusCode, ApiErr> {
    auth::require(&c, "tuya.write")?;
    let tid = tenant_of(&c)?;
    let access_id = b.access_id.trim().to_string();
    let access_key = b.access_key.trim().to_string();
    let endpoint = normalise_endpoint(Some(&b.endpoint));
    if access_id.is_empty() || access_key.is_empty() {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "Access ID and Access Secret are required",
        ));
    }
    let cfg = TuyaConfig {
        access_id,
        access_key,
        endpoint,
        // Saved separately by `PUT /tuya/console`, and NOT cleared here: re-entering credentials
        // must not silently drop the console link the tenant already gave us.
        console_project_id: None,
        console_region: None,
    };
    let db = st.db.clone();
    blocking(move || -> Result<(), String> {
        tuya::client::validate(&cfg)?; // wrong keys / data center / unsubscribed API → clear error
        tuya::repo::save_config(&db, tid, &cfg)
    })
    .await?
    .map_err(|e| {
        err(
            StatusCode::BAD_REQUEST,
            format!("Couldn't connect to Tuya: {e}"),
        )
    })?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct DiagnoseReq {
    /// Credentials to probe. Omitted → probe the ones already saved, which is how the connected
    /// state re-checks itself without making anyone re-type a secret.
    #[serde(default)]
    access_id: Option<String>,
    #[serde(default)]
    access_key: Option<String>,
    #[serde(default)]
    endpoint: Option<String>,
}

/// Say WHY a Tuya connection is not working, as a four-step checklist (#182).
///
/// Deliberately answers `200` with a diagnosis rather than an error status: "your project is in
/// another data centre" is a successful diagnosis, not a failed request, and the UI needs the whole
/// struct — the suggested endpoint especially — which an `ApiErr` (a bare `(Status, String)`) has
/// nowhere to carry.
async fn diagnose(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
    Json(b): Json<DiagnoseReq>,
) -> Result<Json<tuya::Diagnosis>, ApiErr> {
    auth::require(&c, "tuya.write")?;
    let tid = tenant_of(&c)?;
    let db = st.db.clone();
    let d = blocking(move || -> Result<tuya::Diagnosis, String> {
        let cfg = match (b.access_id, b.access_key) {
            (Some(id), Some(key)) if !id.trim().is_empty() && !key.trim().is_empty() => TuyaConfig {
                access_id: id.trim().to_string(),
                access_key: key.trim().to_string(),
                endpoint: normalise_endpoint(b.endpoint.as_deref()),
                console_project_id: None,
                console_region: None,
            },
            // No credentials in the request: diagnose what is saved. `resolve_config` is the one
            // resolver (see the module header) — do not read the table directly here.
            _ => tuya::repo::resolve_config(&db, tid).ok_or(
                "Tuya is not connected — add your Tuya project credentials first.",
            )?,
        };
        let uids = tuya::repo::linked_uids(&db, tid);
        Ok(tuya::diagnose(&cfg, &uids))
    })
    .await?
    .map_err(|e| err(StatusCode::BAD_REQUEST, e))?;
    Ok(Json(d))
}

#[derive(Deserialize)]
struct TestReq {
    /// The function to exercise. Omitted → read the device's status and change nothing.
    #[serde(default)]
    code: Option<String>,
    /// The value to send. Only honoured with `code`, and the UI confirms first — the device may be
    /// a lock or a heater, and a "just checking" click must not open a door (#176's reasoning).
    #[serde(default)]
    value: Option<Value>,
}

/// Prove the device actually answers — the moment someone believes the integration works.
///
/// Reads status by default (changes nothing). With `code` + `value` it sends one command. Reports
/// the round-trip in milliseconds either way: a Tuya cloud call is 300-800 ms, and seeing that
/// number sets the right expectation before anyone builds a workflow on it. It is also the
/// measured argument for local control (#133) rather than an asserted one.
async fn test_device(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
    Path(device_id): Path<String>,
    Json(b): Json<TestReq>,
) -> Result<Json<Value>, ApiErr> {
    auth::require(&c, "tuya.write")?;
    let tid = tenant_of(&c)?;
    let db = st.db.clone();
    let out = blocking(move || -> Result<Value, String> {
        let cfg = tuya::repo::resolve_config(&db, tid)
            .ok_or("Tuya is not connected — add your Tuya project credentials first.")?;
        let started = std::time::Instant::now();
        let result = match (&b.code, &b.value) {
            (Some(code), Some(value)) => {
                tuya::client::send_command(&cfg, &device_id, code, value)?;
                json!({ "sent": { "code": code, "value": value } })
            }
            _ => {
                let status = tuya::client::get_status(&cfg, &device_id)?;
                json!({ "status": status.into_iter().collect::<serde_json::Map<_, _>>() })
            }
        };
        let ms = started.elapsed().as_millis() as u64;
        Ok(json!({ "ok": true, "ms": ms, "result": result }))
    })
    .await?
    .map_err(|e| err(StatusCode::BAD_GATEWAY, e))?;
    Ok(Json(out))
}

/// The saved-or-default data-centre base URL. One place, so `set_config` and `diagnose` cannot
/// disagree about what a blank endpoint means.
fn normalise_endpoint(raw: Option<&str>) -> String {
    let e = raw.unwrap_or("").trim().trim_end_matches('/');
    if e.is_empty() {
        "https://openapi.tuyaus.com".to_string()
    } else {
        e.to_string()
    }
}

/// Disconnect the tenant's Tuya account (removes their saved credentials).
async fn delete_config(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
) -> Result<StatusCode, ApiErr> {
    auth::require(&c, "tuya.write")?;
    let tid = tenant_of(&c)?;
    let db = st.db.clone();
    blocking(move || tuya::repo::delete_config(&db, tid))
        .await?
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(StatusCode::NO_CONTENT)
}

/// Pull the tenant's linked devices from the Tuya Cloud and upsert them, using the tenant's own
/// (or the operator fallback) credentials.
async fn sync(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
) -> Result<Json<Value>, ApiErr> {
    auth::require(&c, "tuya.read")?;
    let tid = tenant_of(&c)?;
    let db = st.db.clone();
    let synced = blocking(move || -> Result<usize, String> {
        let cfg = tuya::repo::resolve_config(&db, tid).ok_or(
            "Tuya is not connected — add your Tuya project credentials in Settings → Integrations.",
        )?;
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
async fn provider(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
) -> Result<Json<Value>, ApiErr> {
    auth::require(&c, "tuya.read")?;
    let tid = tenant_of(&c)?;
    let db = st.db.clone();
    let cfg = blocking(move || tuya::repo::load_config(&db, tid)).await?;
    let connected = cfg.is_some();
    let source = if connected { "tenant" } else { "none" };
    // Where to send someone who wants to link ANOTHER Smart Life account. Always present once
    // credentials exist; sharper once the tenant has pasted a console URL. `exact` says which,
    // so the UI can offer the paste field only when it would actually improve the link.
    let (console_url, exact) = match &cfg {
        Some(c) => (
            Some(tuya::domain::console_link_url(c)),
            c.console_project_id.is_some(),
        ),
        None => (None, false),
    };
    Ok(Json(
        json!({ "configured": connected, "source": source, "console_url": console_url, "console_url_exact": exact }),
    ))
}

#[derive(serde::Deserialize)]
struct ConsoleUrlReq {
    url: String,
}

/// Remember which console project this tenant's credentials belong to, from a URL they pasted.
///
/// Not a credential — it is the identity in the address bar of a console the tenant is already
/// logged into — but it is still tenant data, so it is scoped and permissioned like the rest.
async fn set_console(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
    Json(b): Json<ConsoleUrlReq>,
) -> Result<StatusCode, ApiErr> {
    auth::require(&c, "tuya.write")?;
    let tid = tenant_of(&c)?;
    let Some((project_id, region)) = tuya::domain::parse_console_url(&b.url) else {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "That does not look like a Tuya console project URL — it needs an `id=p…` parameter",
        ));
    };
    let db = st.db.clone();
    blocking(move || tuya::repo::set_console_project(&db, tid, &project_id, region.as_deref()))
        .await?
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(StatusCode::NO_CONTENT)
}
