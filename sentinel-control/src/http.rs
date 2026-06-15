//! Control-plane REST: auth + RBAC + master console (see `handlers`), the edge
//! camera-config pull / discovery push, the edge installer + binaries, and health.
use axum::extract::State;
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Json, Response};
use axum::routing::{get, post};
use axum::Router;
use serde_json::json;
use sha2::{Digest, Sha256};
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;

use app_core::devices;
use app_core::onvif::DiscoveredDevice;
use app_core::tenancy::DEFAULT_TENANT_ID;

use crate::{handlers, store, AppState};

pub fn router(state: AppState) -> Router {
    // Edge appliance binaries (the repo is private, so they're served here, publicly,
    // instead of GitHub releases). CI ships them to EDGE_DIST_DIR on the VPS.
    let edge_dist = std::env::var("EDGE_DIST_DIR").unwrap_or_else(|_| "/edge-dist".to_string());

    Router::new()
        .route("/health", get(health))
        .route("/install.sh", get(install_script))
        .route("/edge-latest", get(edge_latest))
        .route("/config/cameras", get(cameras))
        .route("/config/discovered", post(discovered_push))
        .merge(handlers::router())
        .merge(crate::domain::router())
        .nest_service("/edge", ServeDir::new(edge_dist))
        .layer(CorsLayer::permissive())
        .with_state(state)
}

/// Serve the edge appliance installer so a tenant admin's one-liner works.
async fn install_script() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/x-shellscript; charset=utf-8")],
        include_str!("../../sentinel-edge/install.sh"),
    )
}

/// The latest edge build the operator has published (for the "update available" badge).
async fn edge_latest() -> impl IntoResponse {
    let version = std::env::var("LATEST_EDGE_VERSION").unwrap_or_else(|_| "0.1.0".to_string());
    Json(json!({ "version": version }))
}

async fn health(State(st): State<AppState>) -> impl IntoResponse {
    let n = devices::repo::load(&st.db, DEFAULT_TENANT_ID).len();
    Json(json!({ "status": "ok", "service": "sentinel-control", "cameras": n }))
}

/// Resolve a box token (Authorization: Bearer) → its tenant, stamping last_seen, the
/// reported tailnet host (`X-Box-Host`), and the edge version (`X-Edge-Version`).
/// `Ok(None)` = no token presented; `Err(resp)` = a token was presented but invalid.
async fn box_tenant(st: &AppState, headers: &HeaderMap) -> Result<Option<uuid::Uuid>, Response> {
    let token = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let Some(tok) = token else { return Ok(None) };

    let host = headers.get("x-box-host").and_then(|v| v.to_str().ok());
    let version = headers.get("x-edge-version").and_then(|v| v.to_str().ok());
    let hash = hex::encode(Sha256::digest(tok.as_bytes()));
    match store::touch_box_by_token(&st.pool, &hash, host, version).await {
        Ok(Some(tid)) => Ok(Some(tid)),
        Ok(None) => Err((StatusCode::UNAUTHORIZED, "unknown box token").into_response()),
        Err(e) => {
            log::error!("box token lookup failed: {e}");
            Err((StatusCode::INTERNAL_SERVER_ERROR, "lookup failed").into_response())
        }
    }
}

/// The camera list an edge box pulls, scoped to the box's tenant (box token required —
/// this returns RTSP credentials and control is public). A dev box can opt into the
/// default tenant with `ALLOW_UNTENANTED_CONFIG=1`.
async fn cameras(State(st): State<AppState>, headers: HeaderMap) -> Response {
    let tenant = match box_tenant(&st, &headers).await {
        Ok(Some(tid)) => tid,
        Ok(None) => {
            let dev = std::env::var("ALLOW_UNTENANTED_CONFIG")
                .map(|v| matches!(v.trim(), "1" | "true" | "yes"))
                .unwrap_or(false);
            if dev {
                DEFAULT_TENANT_ID
            } else {
                return (StatusCode::UNAUTHORIZED, "box token required").into_response();
            }
        }
        Err(resp) => return resp,
    };
    Json(devices::repo::load(&st.db, tenant)).into_response()
}

/// The edge pushes its latest ONVIF discovery sweep here (box token required); control
/// stores it tenant-scoped for the cloud Devices page.
async fn discovered_push(State(st): State<AppState>, headers: HeaderMap, body: Option<Json<Vec<DiscoveredDevice>>>) -> Response {
    let tenant = match box_tenant(&st, &headers).await {
        Ok(Some(tid)) => tid,
        Ok(None) => return (StatusCode::UNAUTHORIZED, "box token required").into_response(),
        Err(resp) => return resp,
    };
    let cams = body.map(|Json(c)| c).unwrap_or_default();
    match store::upsert_discovered(&st.pool, tenant, &cams).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => {
            log::error!("upsert_discovered failed: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, "store failed").into_response()
        }
    }
}
