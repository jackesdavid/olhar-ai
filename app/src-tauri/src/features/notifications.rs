//! Notifications — Tauri adapter for the notification slice: Telegram config, the
//! paired beacons, and the inbound pairing poller. The domain + service live in
//! `app_core::notification`; persistence in `app_core::notification::repo`.
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use tauri::{Emitter, Manager};

use app_core::db::Db;
use app_core::notification::{
    self, load_targets, NotificationConfig, NotificationStore, NotificationTarget,
};
use app_core::tenancy::DEFAULT_TENANT_ID;

use crate::db_ref;

/// The current pairing poller's stop flag, so a config change can restart it without
/// an app restart. Managed Tauri state.
pub type PairingHandle = Arc<Mutex<Option<Arc<AtomicBool>>>>;

/// Persisted Telegram config (bot token + pairing code) — the on-disk form the app
/// owns; mirrors `NotificationConfig`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct StoredTelegramConfig {
    #[serde(default)]
    bot_token: String,
    #[serde(default)]
    pairing_code: String,
}

/// What the UI needs about Telegram config WITHOUT leaking the token.
#[derive(Debug, Clone, Serialize)]
pub struct TelegramConfigState {
    configured: bool,
    pairing_code: String,
}

fn notifications_file(app: &tauri::AppHandle) -> std::path::PathBuf {
    let dir = app.path().app_data_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let _ = std::fs::create_dir_all(&dir);
    dir.join("notifications.json")
}

fn telegram_file(app: &tauri::AppHandle) -> std::path::PathBuf {
    let dir = app.path().app_data_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let _ = std::fs::create_dir_all(&dir);
    dir.join("telegram.json")
}

/// Load the saved Telegram config from Postgres. `None` if unset or DB not ready.
fn load_stored_telegram(app: &tauri::AppHandle) -> Option<StoredTelegramConfig> {
    let (bot_token, pairing_code) = notification::repo::load_telegram(&db_ref(app).ok()?, DEFAULT_TENANT_ID)?;
    Some(StoredTelegramConfig { bot_token, pairing_code })
}

/// Resolve the active `NotificationConfig`: the saved config if it has a non-empty
/// token, else the environment. `None` when neither provides a token. `pub(crate)`
/// so the alerts slice (deliver_alert) can resolve the same config.
pub(crate) fn resolve_telegram_config(app: &tauri::AppHandle) -> Option<NotificationConfig> {
    if let Some(stored) = load_stored_telegram(app) {
        if !stored.bot_token.trim().is_empty() {
            return Some(NotificationConfig {
                bot_token: stored.bot_token,
                pairing_code: stored.pairing_code,
            });
        }
    }
    NotificationConfig::from_env()
}

/// (Re)start the Telegram pairing poller: signal any running one to stop, then start
/// a fresh poller if Telegram is configured with a pairing code.
fn restart_pairing(app: &tauri::AppHandle, store: NotificationStore, handle: &PairingHandle) {
    if let Some(old) = handle.lock().unwrap().take() {
        old.store(true, Ordering::Relaxed); // tell the previous poller to exit
    }
    match resolve_telegram_config(app) {
        Some(cfg) if cfg.can_pair() => {
            let stop = Arc::new(AtomicBool::new(false));
            *handle.lock().unwrap() = Some(stop.clone());
            let emitter = app.clone();
            notification::spawn_pairing(store, cfg, db_ref(app).ok(), DEFAULT_TENANT_ID, stop, move || {
                let _ = emitter.emit("beacons-changed", ());
            });
            log::info!("Pairing poller (re)started.");
        }
        Some(_) => log::info!("Telegram token set but no pairing code — poller not started."),
        None => log::info!("Telegram not configured — poller not started."),
    }
}

// ---- lifecycle (called by lib.rs setup / try_connect_db) --------------------

/// Manage the beacon store + pairing handle empty, BEFORE the DB connect.
pub fn manage(app: &tauri::AppHandle) {
    let notif_store: NotificationStore = Arc::new(Mutex::new(Vec::new()));
    app.manage(notif_store);
    let pairing: PairingHandle = Arc::new(Mutex::new(None));
    app.manage(pairing);
}

/// Hydrate beacons from Postgres (with a one-time import from legacy
/// notifications.json) and import any legacy telegram.json config.
pub fn hydrate(app: &tauri::AppHandle, db: &Db) {
    let mut beacons = notification::repo::load_beacons(db, DEFAULT_TENANT_ID);
    if beacons.is_empty() {
        let from_json = load_targets(&notifications_file(app));
        if !from_json.is_empty() {
            notification::repo::save_beacons(db, DEFAULT_TENANT_ID, &from_json);
            log::info!("Imported {} beacon(s) from notifications.json into Postgres", from_json.len());
            beacons = notification::repo::load_beacons(db, DEFAULT_TENANT_ID);
        }
    }
    log::info!("Beacons loaded from Postgres — {} target(s)", beacons.len());
    *app.state::<NotificationStore>().lock().unwrap() = beacons;

    if notification::repo::load_telegram(db, DEFAULT_TENANT_ID).is_none() {
        if let Some(stored) = std::fs::read_to_string(telegram_file(app))
            .ok()
            .and_then(|raw| serde_json::from_str::<StoredTelegramConfig>(&raw).ok())
        {
            if !stored.bot_token.trim().is_empty() || !stored.pairing_code.trim().is_empty() {
                notification::repo::save_telegram(db, DEFAULT_TENANT_ID, &stored.bot_token, &stored.pairing_code);
                log::info!("Imported Telegram config from telegram.json into Postgres");
            }
        }
    }
}

/// Start the pairing poller from the managed state (called once at setup).
pub fn start_pairing(app: &tauri::AppHandle) {
    let store = app.state::<NotificationStore>().inner().clone();
    let handle = app.state::<PairingHandle>().inner().clone();
    restart_pairing(app, store, &handle);
}

// ---- commands ---------------------------------------------------------------

/// Report whether Telegram is configured + the pairing code (never the token).
#[tauri::command]
pub fn get_telegram_config(app: tauri::AppHandle) -> TelegramConfigState {
    match resolve_telegram_config(&app) {
        Some(cfg) => TelegramConfigState {
            configured: !cfg.bot_token.trim().is_empty(),
            pairing_code: cfg.pairing_code,
        },
        None => TelegramConfigState { configured: false, pairing_code: String::new() },
    }
}

/// Persist the Telegram bot token + pairing code, then restart the pairing poller.
#[tauri::command]
pub fn set_telegram_config(
    app: tauri::AppHandle,
    notif: tauri::State<'_, NotificationStore>,
    pairing: tauri::State<'_, PairingHandle>,
    bot_token: String,
    pairing_code: String,
) -> Result<String, String> {
    // Blank token field means "keep the saved one" — the token is never sent to the UI.
    let token = {
        let t = bot_token.trim();
        if t.is_empty() {
            load_stored_telegram(&app).map(|s| s.bot_token).unwrap_or_default()
        } else {
            t.to_string()
        }
    };
    let cfg = StoredTelegramConfig { bot_token: token, pairing_code: pairing_code.trim().to_string() };
    notification::repo::save_telegram(&db_ref(&app)?, DEFAULT_TENANT_ID, &cfg.bot_token, &cfg.pairing_code);
    log::info!(
        "set_telegram_config: token={} pairing_code={}",
        if cfg.bot_token.is_empty() { "unset" } else { "set" },
        if cfg.pairing_code.is_empty() { "unset" } else { "set" }
    );
    restart_pairing(&app, notif.inner().clone(), &pairing);
    Ok("Saved — pairing listener restarted.".into())
}

/// The paired beacons (Telegram targets).
#[tauri::command]
pub fn list_beacons(state: tauri::State<'_, NotificationStore>) -> Vec<NotificationTarget> {
    state.lock().unwrap().clone()
}

/// Enable/disable a beacon by `chat_id` and persist.
#[tauri::command]
pub fn toggle_beacon(
    app: tauri::AppHandle,
    state: tauri::State<'_, NotificationStore>,
    chat_id: i64,
    enabled: bool,
) -> Result<(), String> {
    let (label, _) = notification::set_enabled_by_id(&state, db_ref(&app).ok().as_ref(), DEFAULT_TENANT_ID, chat_id, enabled)
        .ok_or("Beacon not found.")?;
    log::info!("toggle_beacon: '{label}' (chat_id={chat_id}) enabled={enabled}");
    Ok(())
}

/// Rename a beacon by `chat_id` and persist.
#[tauri::command]
pub fn rename_beacon(
    app: tauri::AppHandle,
    state: tauri::State<'_, NotificationStore>,
    chat_id: i64,
    label: String,
) -> Result<(), String> {
    let new_label = notification::rename_target(&state, db_ref(&app).ok().as_ref(), DEFAULT_TENANT_ID, chat_id, &label)?;
    log::info!("rename_beacon: chat_id={chat_id} -> '{new_label}'");
    Ok(())
}

/// Remove a beacon by `chat_id` and persist.
#[tauri::command]
pub fn delete_beacon(
    app: tauri::AppHandle,
    state: tauri::State<'_, NotificationStore>,
    chat_id: i64,
) -> Result<(), String> {
    let removed = notification::delete_target(&state, db_ref(&app).ok().as_ref(), DEFAULT_TENANT_ID, chat_id)
        .ok_or("Beacon not found.")?;
    log::info!("delete_beacon: removed '{removed}' (chat_id={chat_id})");
    Ok(())
}

/// Send a test message to the beacon with `label`. Network IO on a blocking task.
#[tauri::command]
pub async fn test_beacon(
    app: tauri::AppHandle,
    state: tauri::State<'_, NotificationStore>,
    label: String,
) -> Result<String, String> {
    let cfg = resolve_telegram_config(&app)
        .ok_or("Telegram is not configured (set the bot token in Beacons).")?;
    let store = state.inner().clone();
    let msg = "\u{1F514} Test from Sentinel — this beacon is live.".to_string();
    let result = tauri::async_runtime::spawn_blocking(move || {
        notification::dispatch(&store, &cfg, Some(&label), &msg, None)
    })
    .await
    .map_err(|e| format!("Test task failed: {e}"))?;
    match &result {
        Ok(s) => log::info!("test_beacon: {s}"),
        Err(e) => log::error!("test_beacon failed: {e}"),
    }
    result
}
