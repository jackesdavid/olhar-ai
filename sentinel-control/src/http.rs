//! Control-plane REST: auth + RBAC + master console (see `handlers`), plus the edge
//! camera-config pull and health.
use axum::extract::State;
use axum::response::{IntoResponse, Json};
use axum::routing::get;
use axum::Router;
use serde_json::json;
use tower_http::cors::CorsLayer;

use app_core::devices;
use app_core::tenancy::DEFAULT_TENANT_ID;

use crate::{handlers, AppState};

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

/// The camera list an edge box pulls. (Phase 2 scopes this by the box's tenant.)
async fn cameras(State(st): State<AppState>) -> impl IntoResponse {
    Json(devices::repo::load(&st.db, DEFAULT_TENANT_ID))
}
