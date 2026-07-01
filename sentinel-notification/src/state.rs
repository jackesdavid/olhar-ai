//! Shared service state — the Postgres handle (queue + config live there) plus the
//! per-tenant Telegram pairing-poller registry (this service owns pairing now).
use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use app_core::db::Db;
use uuid::Uuid;

/// tenant → (its bot token, stop flag) for the running pairing pollers.
pub type PairingStops = Arc<Mutex<HashMap<Uuid, (String, Arc<AtomicBool>)>>>;

pub struct AppState {
    pub db: Db,
    pub pairing_stops: PairingStops,
}

impl AppState {
    /// Connect to Postgres (runs the embedded migrations) and build the state.
    pub fn boot(database_url: &str) -> Result<Arc<AppState>, String> {
        let db = Db::connect(database_url).map_err(|e| e.to_string())?;
        Ok(Arc::new(AppState {
            db,
            pairing_stops: Arc::new(Mutex::new(HashMap::new())),
        }))
    }
}
