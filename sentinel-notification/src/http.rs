//! HTTP surface. For now just `/health` (compose healthcheck); the config REST + read-only
//! observability endpoints land in later phases.
use std::sync::Arc;

use axum::{routing::get, Router};

use crate::state::AppState;

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/health", get(health))
        .with_state(state)
}

async fn health() -> &'static str {
    "ok"
}
