//! Telegram pairing-poller lifecycle — moved here from sentinel-flow (card #57): the
//! notification service now owns pairing (inbound) alongside sending (outbound). One long-poll
//! poller per tenant with a bot token; reconciled every 60s so config changes take effect
//! without a restart.
//!
//! Note: `on_change` is a no-op here (the paired beacon still persists to Postgres and shows on
//! the Recipients page on next load). Live SSE refresh-on-pair would need a Redis publish; a
//! follow-up if we want it back.
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use app_core::notification;
use uuid::Uuid;

use crate::state::AppState;

/// Spawn all pollers from current DB state, then reconcile every 60s.
pub fn spawn(state: Arc<AppState>) {
    sync(&state);
    let s = state.clone();
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(60));
        sync(&s);
    });
    log::info!("Telegram pairing pollers started");
}

/// Reconcile running pollers against the DB: start/restart on a new-or-changed token, stop
/// pollers whose token was cleared.
fn sync(state: &Arc<AppState>) {
    let configs = notification::repo::load_all_telegram(&state.db);
    let active: HashSet<Uuid> = configs.iter().map(|(t, _)| *t).collect();

    {
        let mut stops = state.pairing_stops.lock().unwrap();
        let removed: Vec<Uuid> = stops.keys().filter(|t| !active.contains(*t)).copied().collect();
        for t in removed {
            let (_, flag) = stops.remove(&t).unwrap();
            flag.store(true, Ordering::Relaxed);
            log::info!("pairing poller stopped for tenant {t} (config removed)");
        }
    }

    for (tenant, cfg) in configs {
        let needs_start = {
            let stops = state.pairing_stops.lock().unwrap();
            match stops.get(&tenant) {
                None => true,
                Some((saved_token, _)) => saved_token != &cfg.bot_token,
            }
        };
        if needs_start {
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

    let store = Arc::new(std::sync::Mutex::new(notification::repo::load_beacons(&state.db, tenant)));
    let db = state.db.clone();
    notification::spawn_pairing(store, cfg, Some(db), tenant, stop, || {});
    log::info!("pairing poller started for tenant {tenant}");
}
