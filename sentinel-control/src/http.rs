//! Control-plane REST: auth + RBAC + master console (see `handlers`), plus the edge
//! camera-config pull and health.
use axum::extract::State;
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Json};
use axum::routing::get;
use axum::Router;
use serde_json::json;
use sha2::{Digest, Sha256};
use tower_http::cors::CorsLayer;

use app_core::devices;
use app_core::tenancy::DEFAULT_TENANT_ID;

use crate::{handlers, store, AppState};

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/config/cameras", get(cameras))
        .merge(handlers::router())
        .merge(crate::domain::router())
        .layer(CorsLayer::permissive())
        .with_state(state)
}

async fn health(State(st): State<AppState>) -> impl IntoResponse {
    let n = devices::repo::load(&st.db, DEFAULT_TENANT_ID).len();
    Json(json!({ "status": "ok", "service": "sentinel-control", "cameras": n }))
}

/// The camera list an edge box pulls, scoped to the box's tenant. The edge presents
/// its box token as `Authorization: Bearer <token>` and (optionally) its tailnet host
/// as `X-Box-Host`. A valid token → that tenant's cameras (and last_seen/host are
/// stamped); an unknown token → 401; no token → the default tenant (single-tenant /
/// dev edges that predate enrollment).
async fn cameras(State(st): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    let token = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|s| s.trim().to_string());

    let tenant = match token {
        Some(tok) if !tok.is_empty() => {
            let host = headers.get("x-box-host").and_then(|v| v.to_str().ok());
            let hash = hex::encode(Sha256::digest(tok.as_bytes()));
            match store::touch_box_by_token(&st.pool, &hash, host).await {
                Ok(Some(tid)) => tid,
                Ok(None) => return (StatusCode::UNAUTHORIZED, "unknown box token").into_response(),
                Err(e) => {
                    log::error!("box token lookup failed: {e}");
                    return (StatusCode::INTERNAL_SERVER_ERROR, "lookup failed").into_response();
                }
            }
        }
        _ => DEFAULT_TENANT_ID,
    };
    Json(devices::repo::load(&st.db, tenant)).into_response()
}
