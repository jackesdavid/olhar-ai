//! Telegram pairing-poller lifecycle. Spawns one long-poll poller per tenant
//! that has a bot token set, and reconciles running pollers every 60 seconds
//! so config changes made via sentinel-control take effect without a restart.

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use uuid::Uuid;

use app_core::notification;

use crate::events;
use crate::state::AppState;

/// Spawn all pollers from current DB state, then launch a background sync
/// thread that reconciles running pollers every 60 seconds.
pub fn spawn(state: Arc<AppState>) {
    sync(&state);
    let s = state.clone();
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(60));
        sync(&s);
    });
}

/// Reconcile running pollers against the DB. For each tenant with a non-empty
/// token, start a fresh poller if none is running or the token has changed.
/// Pollers for tenants whose token was cleared are stopped via their stop flag.
pub fn sync(state: &Arc<AppState>) {
    let configs = notification::repo::load_all_telegram(&state.db);
    let active_tenants: HashSet<Uuid> = configs.iter().map(|(t, _)| *t).collect();

    // Stop pollers for tenants whose config was removed or token cleared.
    {
        let mut stops = state.pairing_stops.lock().unwrap();
        let removed: Vec<Uuid> = stops.keys().filter(|t| !active_tenants.contains(*t)).copied().collect();
        for t in removed {
            let (_, flag) = stops.remove(&t).unwrap();
            flag.store(true, Ordering::Relaxed);
            log::info!("pairing poller stopped for tenant {t} (config removed)");
        }
    }

    // Start or restart pollers as needed.
    for (tenant, cfg) in configs {
        let needs_start = {
            let stops = state.pairing_stops.lock().unwrap();
            match stops.get(&tenant) {
                None => true,
                Some((saved_token, _)) => saved_token != &cfg.bot_token,
            }
        };
        if needs_start {
            // Retire the old poller if there is one (token changed).
            if let Some((_, flag)) = state.pairing_stops.lock().unwrap().remove(&tenant) {
                flag.store(true, Ordering::Relaxed);
            }
            start_tenant_poller(state, tenant, cfg);
        }
    }
}

fn start_tenant_poller(state: &Arc<AppState>, tenant: Uuid, cfg: notification::NotificationConfig) {
    let stop = Arc::new(AtomicBool::new(false));
    state
        .pairing_stops
        .lock()
        .unwrap()
        .insert(tenant, (cfg.bot_token.clone(), Arc::clone(&stop)));

    let store = Arc::new(std::sync::Mutex::new(
        notification::repo::load_beacons(&state.db, tenant),
    ));
    let db = state.db.clone();
    let bus = state.events.clone();

    notification::spawn_pairing(store, cfg, Some(db), tenant, stop, move || {
        events::publish(&bus, "beacons-changed", serde_json::json!({ "tenant": tenant }));
    });
    log::info!("pairing poller started for tenant {tenant}");
}
