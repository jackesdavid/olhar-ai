//! Settings — Tauri adapter for the app-settings slice. Settings are UI-local
//! (stored under the platform app-config dir, not shared via Postgres). Changing
//! `cameras_always_warm` re-reconciles the `stream` slice's keep-warm poller live.
use std::path::PathBuf;

use tauri::Manager;

use app_core::config::{load_settings, save_settings, Settings};

/// The web app's settings file, under the platform app-config dir (created if missing).
/// `pub(crate)` so the `stream` slice's keep-warm poller can read `cameras_always_warm`.
pub(crate) fn settings_file(app: &tauri::AppHandle) -> PathBuf {
    let dir = app
        .path()
        .app_config_dir()
        .unwrap_or_else(|_| PathBuf::from("."));
    let _ = std::fs::create_dir_all(&dir);
    dir.join("settings.json")
}

/// The current settings (defaults if the file is absent).
#[tauri::command]
pub fn get_settings(app: tauri::AppHandle) -> Settings {
    load_settings(&settings_file(&app))
}

/// Persist settings, then react to the "keep camera feeds always on" toggle
/// without an app restart.
#[tauri::command]
pub fn set_settings(app: tauri::AppHandle, settings: Settings) {
    save_settings(&settings_file(&app), &settings);
    crate::features::stream::reconcile_keepwarm(&app);
}
