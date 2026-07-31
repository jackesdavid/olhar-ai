//! bridged (MSE over WebSocket); everything else GET is streamed through (snapshots,
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
        // Live-over-outbound (card #144): the edge dials THIS to push a stream up its own connection.
        .route("/edge/live/:session", get(proxy::edge_live))
        .fallback(proxy::passthrough) // /api/frame.jpeg, /api/streams, /stream.html, assets…
        .layer(CorsLayer::permissive())
        .with_state(state)
}

/// Liveness only. Deliberately says nothing about topology: this endpoint is public and
/// unauthenticated, and it used to publish `upstream` — the internal edge host — to anyone who
/// asked, which is how an address that should never have been discoverable became a scan target.
/// A health check answers "am I up", not "what is behind me".
async fn health() -> impl IntoResponse {
    Json(json!({ "status": "ok", "service": "sentinel-gateway" }))
}

/// `GET /player?src=cam_<ip>` → `/stream.html?src=cam_<ip>&mode=mse`.
async fn player(RawQuery(query): RawQuery) -> impl IntoResponse {
    let q = query.unwrap_or_default();
    let sep = if q.is_empty() { "" } else { "&" };
    Redirect::temporary(&format!("/stream.html?{q}{sep}mode=mse"))
}
