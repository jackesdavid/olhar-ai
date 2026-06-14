//! The gateway router. A scoped reverse proxy to the upstream go2rtc: `/api/ws` is
//! bridged (MSE over WebSocket); everything else GET is streamed through (snapshots,
//! stream list, and go2rtc's own player assets), so the bundled MSE player works
//! through the gateway with minimal code.
use std::sync::Arc;

use axum::extract::RawQuery;
use axum::response::{IntoResponse, Json, Redirect};
use axum::routing::get;
use axum::Router;
use serde_json::json;
use tower_http::cors::CorsLayer;

use crate::proxy;
use crate::state::AppState;

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/player", get(player))
        .route("/api/ws", get(proxy::ws_handler))
        .fallback(proxy::passthrough) // /api/frame.jpeg, /api/streams, /stream.html, assets…
        .layer(CorsLayer::permissive())
        .with_state(state)
}

async fn health(axum::extract::State(state): axum::extract::State<Arc<AppState>>) -> impl IntoResponse {
    Json(json!({ "status": "ok", "service": "sentinel-gateway", "upstream": state.resolve_upstream() }))
}

/// Convenience: open go2rtc's player (proxied) forced to MSE for a camera.
/// `GET /player?src=cam_<ip>` → `/stream.html?src=cam_<ip>&mode=mse`.
async fn player(RawQuery(query): RawQuery) -> impl IntoResponse {
    let q = query.unwrap_or_default();
    let sep = if q.is_empty() { "" } else { "&" };
    Redirect::temporary(&format!("/stream.html?{q}{sep}mode=mse"))
}
