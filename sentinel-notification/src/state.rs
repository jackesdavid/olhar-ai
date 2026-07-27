//! Shared service state — the Postgres handle (queue + config live there) plus the
//! Telegram pairing-poller registry (this service owns pairing now).
use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use app_core::db::Db;

/// bot token → (tenant/code-set signature, stop flag) for the running pairing pollers.
/// Keyed by TOKEN, not tenant: Telegram permits only one `getUpdates` long-poll per token, so
/// tenants that share a token (e.g. both on the operator's global bot) MUST share ONE poller — a
/// second poller on the same token gets HTTP 409 and the two race, splitting/dropping messages.
/// The signature is the sorted `(tenant, pairing_code)` set; a change (new tenant, rotated code)
/// restarts the poller so it picks up the new routing/codes.
pub type PairingStops = Arc<Mutex<HashMap<String, (String, Arc<AtomicBool>)>>>;

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
