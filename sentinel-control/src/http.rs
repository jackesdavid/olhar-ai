//! Control-plane REST. The edge appliance pulls its camera config from here.
use std::sync::Arc;

use axum::extract::State;
use axum::response::{IntoResponse, Json};
use axum::routing::get;
use axum::Router;
use serde_json::json;
use tower_http::cors::CorsLayer;

use app_core::devices;

use crate::AppState;

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/config/cameras", get(cameras))
        .layer(CorsLayer::permissive())
        .with_state(state)
}

async fn health(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let n = devices::repo::load(&state.db).len();
    Json(json!({ "status": "ok", "service": "sentinel-control", "cameras": n }))
}

/// The full camera list (incl. creds + rtsp_url) for an edge box to build its go2rtc
/// config and serve snapshots. The edge is trusted (on-prem, over the tailnet).
async fn cameras(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let cameras = devices::repo::load(&state.db);
    log::info!("config/cameras → {} camera(s)", cameras.len());
    Json(cameras)
}
