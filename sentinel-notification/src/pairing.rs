//! Telegram pairing-poller lifecycle — moved here from sentinel-flow (card #57): the
//! notification service now owns pairing (inbound) alongside sending (outbound).
//!
//! ONE poller per **bot token**, not per tenant. Telegram allows only a single `getUpdates`
//! long-poll per token; tenants that resolve to the same token (e.g. both fall back to the
//! operator's global bot) share ONE poller that routes each message to the owning tenant. Running a
//! poller per tenant on a shared token 409s and splits/drops messages (the "only some Telegram
//! messages reach the workflow" bug). Reconciled every 60s so config changes take effect without a
//! restart.
//!
//! Note: pairing persists the beacon to Postgres (shows on the Recipients page on next load); there
//! is no live SSE refresh-on-pair here.
use std::collections::{HashMap, HashSet};
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

/// A stable signature of the tenant/code set behind a token — a change (added tenant, rotated code)
/// respawns the poller so it picks up the new routing/codes.
fn signature(tenants: &[(Uuid, String)]) -> String {
    let mut parts: Vec<String> = tenants.iter().map(|(t, c)| format!("{t}={c}")).collect();
    parts.sort();
    parts.join(",")
}

/// Reconcile running pollers against the DB: group tenants by resolved bot token, start/restart a
/// poller when its token is new or its tenant/code set changed, stop pollers whose token vanished.
fn sync(state: &Arc<AppState>) {
    let configs = notification::repo::load_all_telegram(&state.db);

    // Group by RESOLVED bot token (empty token → no bot, skip). Keep one representative cfg per
    // token for the getUpdates client; collect every tenant's (id, pairing_code) behind it.
    let mut by_token: HashMap<String, Vec<(Uuid, String)>> = HashMap::new();
    let mut cfg_for: HashMap<String, notification::NotificationConfig> = HashMap::new();
    for (tenant, cfg) in configs {
        if cfg.bot_token.trim().is_empty() {
            continue;
        }
        by_token
            .entry(cfg.bot_token.clone())
            .or_default()
            .push((tenant, cfg.pairing_code.clone()));
        cfg_for.entry(cfg.bot_token.clone()).or_insert(cfg);
    }

    let active: HashSet<String> = by_token.keys().cloned().collect();
    {
        let mut stops = state.pairing_stops.lock().unwrap();
        let removed: Vec<String> = stops.keys().filter(|t| !active.contains(*t)).cloned().collect();
        for token in removed {
            let (_, flag) = stops.remove(&token).unwrap();
            flag.store(true, Ordering::Relaxed);
            log::info!("pairing poller stopped (bot config removed)");
        }
    }

    for (token, mut tenants) in by_token {
        tenants.sort_by_key(|(t, _)| *t);
        let sig = signature(&tenants);
        let needs_start = {
            let stops = state.pairing_stops.lock().unwrap();
            match stops.get(&token) {
                None => true,
                Some((saved_sig, _)) => saved_sig != &sig,
            }
        };
        if needs_start {
            if let Some((_, flag)) = state.pairing_stops.lock().unwrap().remove(&token) {
                flag.store(true, Ordering::Relaxed);
            }
            let cfg = cfg_for.remove(&token).expect("cfg for token");
            start_bot_poller(state, token, sig, cfg, tenants);
        }
    }
}

/// Spawn one poller for a bot token shared by `tenants`, tracking it under the token in the registry.
fn start_bot_poller(
    state: &Arc<AppState>,
    token: String,
    sig: String,
    cfg: notification::NotificationConfig,
    tenants: Vec<(Uuid, String)>,
) {
    let stop = Arc::new(AtomicBool::new(false));
    state
        .pairing_stops
        .lock()
        .unwrap()
        .insert(token, (sig, Arc::clone(&stop)));

    let n = tenants.len();
    let db = state.db.clone();
    notification::spawn_pairing_bot(cfg, db, tenants, stop);
    log::info!("pairing poller started for bot ({n} tenant(s))");
}
