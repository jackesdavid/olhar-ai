//! Shared service state — just the Postgres handle (the queue + config live there).
use std::sync::Arc;

use app_core::db::Db;

pub struct AppState {
    pub db: Db,
}

impl AppState {
    /// Connect to Postgres (runs the embedded migrations) and build the state.
    pub fn boot(database_url: &str) -> Result<Arc<AppState>, String> {
        let db = Db::connect(database_url).map_err(|e| e.to_string())?;
        Ok(Arc::new(AppState { db }))
    }
}
