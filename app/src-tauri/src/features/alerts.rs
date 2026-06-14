//! Alerts — Tauri adapter for the alerts slice. Owns the alert `#[tauri::command]`s
//! and the managed-store lifecycle (`manage_empty_store`/`hydrate`). The domain +
//! service live in `app_core::alerts`; persistence in `app_core::alerts::repo`.
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::Manager;

use app_core::alerts::{self, Alert, AlertLevel, AlertStore};
use app_core::db::Db;
use app_core::notification::NotificationStore;
use app_core::tenancy::DEFAULT_TENANT_ID;

use crate::db_ref;
use crate::features::notifications::resolve_telegram_config;

/// The legacy `alerts.json` path (one-time import source on first DB boot only).
fn alerts_file(app: &tauri::AppHandle) -> std::path::PathBuf {
    let dir = app.path().app_data_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let _ = std::fs::create_dir_all(&dir);
    dir.join("alerts.json")
}

// ---- lifecycle (called by lib.rs setup / try_connect_db) --------------------

/// Manage an empty alert store BEFORE the DB connect so [`hydrate`] can fill it.
pub fn manage_empty_store(app: &tauri::AppHandle) {
    let store: AlertStore = Arc::new(Mutex::new(Vec::new()));
    app.manage(store);
}

/// Hydrate the in-memory store from Postgres, with a one-time non-destructive import
/// from a legacy `alerts.json` (ids preserved) when the DB log is empty.
pub fn hydrate(app: &tauri::AppHandle, db: &Db) {
    let mut alerts = alerts::repo::load(db, DEFAULT_TENANT_ID);
    if alerts.is_empty() {
        let from_json = alerts::repo::import_legacy_json(&alerts_file(app));
        if !from_json.is_empty() {
            for a in &from_json {
                alerts::repo::insert(db, DEFAULT_TENANT_ID, a);
            }
            log::info!("Imported {} alert(s) from alerts.json into Postgres", from_json.len());
            alerts = alerts::repo::load(db, DEFAULT_TENANT_ID);
        }
    }
    log::info!("Alerts loaded from Postgres — {} alert(s)", alerts.len());
    *app.state::<AlertStore>().lock().unwrap() = alerts;
}

// ---- commands ---------------------------------------------------------------

/// Aggregate counts for the alerts dashboard.
#[derive(Debug, Clone, Serialize)]
pub struct AlertStats {
    total: u32,
    today: u32,
    by_level: Vec<(String, u32)>,
    undelivered: u32,
}

/// The alert log, newest first.
#[tauri::command]
pub fn list_alerts(state: tauri::State<'_, AlertStore>) -> Vec<Alert> {
    let mut alerts = state.lock().unwrap().clone();
    alerts.sort_by(|a, b| b.created_at.cmp(&a.created_at).then(b.id.cmp(&a.id)));
    alerts
}

/// Create an alert (level defaults to Info, source defaults to "operator"), persist,
/// and return it. `room` is free text stored as `room_name`.
#[tauri::command]
pub fn create_alert(
    app: tauri::AppHandle,
    state: tauri::State<'_, AlertStore>,
    level: String,
    message: String,
    room: Option<String>,
    source: Option<String>,
) -> Alert {
    let level = AlertLevel::parse(&level).unwrap_or_default();
    let source = source
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "operator".to_string());
    let room_name = room.map(|r| r.trim().to_string()).filter(|r| !r.is_empty());

    let alert = alerts::create(&state, db_ref(&app).ok().as_ref(), DEFAULT_TENANT_ID, level, message, None, room_name, source);
    log::info!(
        "create_alert: id={} level={} source={} room={:?}",
        alert.id,
        alert.level.label(),
        alert.source,
        alert.room_name
    );
    alert
}

/// Remove the alert with `id` and persist. Returns whether one was removed.
#[tauri::command]
pub fn delete_alert(app: tauri::AppHandle, state: tauri::State<'_, AlertStore>, id: u64) -> bool {
    let removed = alerts::delete(&state, db_ref(&app).ok().as_ref(), DEFAULT_TENANT_ID, id);
    log::info!("delete_alert: id={id} removed={removed}");
    removed
}

/// Aggregate counts over the alert log for the dashboard stat cards.
#[tauri::command]
pub fn alert_stats(state: tauri::State<'_, AlertStore>) -> AlertStats {
    let alerts = state.lock().unwrap();
    let total = alerts.len() as u32;
    let today = alerts.iter().filter(|a| alerts::is_today(&a.created_at)).count() as u32;
    let undelivered = alerts.iter().filter(|a| a.deliveries.is_empty()).count() as u32;
    let by_level = AlertLevel::ALL
        .iter()
        .map(|&lvl| {
            let n = alerts.iter().filter(|a| a.level == lvl).count() as u32;
            (lvl.label().to_string(), n)
        })
        .collect();
    AlertStats { total, today, by_level, undelivered }
}

/// Deliver the alert with `id` to every enabled beacon and record the result.
/// Network IO on a blocking task.
#[tauri::command]
pub async fn deliver_alert(
    app: tauri::AppHandle,
    alert_state: tauri::State<'_, AlertStore>,
    notif_state: tauri::State<'_, NotificationStore>,
    id: u64,
) -> Result<String, String> {
    let cfg = resolve_telegram_config(&app)
        .ok_or("Telegram is not configured (set the bot token in Beacons).")?;
    let alert_store = alert_state.inner().clone();
    let notif_store = notif_state.inner().clone();
    let db = db_ref(&app).ok();
    let result = tauri::async_runtime::spawn_blocking(move || {
        alerts::deliver(&alert_store, db.as_ref(), DEFAULT_TENANT_ID, &notif_store, &cfg, id)
    })
    .await
    .map_err(|e| format!("Deliver task failed: {e}"))?;
    match &result {
        Ok(s) => log::info!("deliver_alert: {s}"),
        Err(e) => log::error!("deliver_alert failed for id={id}: {e}"),
    }
    result
}
