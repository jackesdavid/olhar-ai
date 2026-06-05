//! Tauri shell — thin `#[tauri::command]` wrappers over `app-core` (Bishop's copied logic).
//! M0 foundation: settings, stored under the OS app-config dir (not the ratatui app's
//! `config.json`).

use app_core::config::{load_settings, save_settings, Settings};
use std::path::PathBuf;
use tauri::Manager;

/// The web app's settings file, under the platform app-config dir (created if missing).
fn settings_file(app: &tauri::AppHandle) -> PathBuf {
    let dir = app
        .path()
        .app_config_dir()
        .unwrap_or_else(|_| PathBuf::from("."));
    let _ = std::fs::create_dir_all(&dir);
    dir.join("settings.json")
}

#[tauri::command]
fn get_settings(app: tauri::AppHandle) -> Settings {
    load_settings(&settings_file(&app))
}

#[tauri::command]
fn set_settings(app: tauri::AppHandle, settings: Settings) {
    save_settings(&settings_file(&app), &settings);
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![get_settings, set_settings])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
