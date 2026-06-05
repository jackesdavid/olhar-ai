//! Tauri shell — thin `#[tauri::command]` wrappers over `app-core` (Bishop's copied logic).
//!
//! M0 foundation: settings, stored under the OS app-config dir (not the ratatui app's
//! `config.json`).
//!
//! Unit 014 — Devices & discovery: a managed `DeviceStore` loaded at `.setup()` from
//! `<app_data_dir>/devices.json`, persisted on every mutation, plus ONVIF discovery,
//! credential validation, and JPEG snapshot commands. Network calls run on blocking
//! tasks so the UI thread never stalls.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use app_core::config::{load_settings, save_settings, Settings};
use app_core::device_actions::capture_snapshot_bytes;
use app_core::devices::{load_devices, save_devices, Device, DeviceStore};
use app_core::onvif;
use app_core::onvif::DiscoveredDevice;
use base64::{engine::general_purpose::STANDARD as B64, Engine};
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

/// The web app's saved-devices file, under the platform app-data dir (created if missing).
fn devices_file(app: &tauri::AppHandle) -> PathBuf {
    let dir = app
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| PathBuf::from("."));
    let _ = std::fs::create_dir_all(&dir);
    dir.join("devices.json")
}

#[tauri::command]
fn get_settings(app: tauri::AppHandle) -> Settings {
    load_settings(&settings_file(&app))
}

#[tauri::command]
fn set_settings(app: tauri::AppHandle, settings: Settings) {
    save_settings(&settings_file(&app), &settings);
}

// ---- Devices ---------------------------------------------------------------

/// A snapshot of the saved device list.
#[tauri::command]
fn list_devices(state: tauri::State<'_, DeviceStore>) -> Vec<Device> {
    state.lock().unwrap().clone()
}

/// Upsert a device by `ip` (replace in place if present, else append) and persist.
#[tauri::command]
fn save_device(app: tauri::AppHandle, state: tauri::State<'_, DeviceStore>, device: Device) {
    {
        let mut devices = state.lock().unwrap();
        if let Some(existing) = devices.iter_mut().find(|d| d.ip == device.ip) {
            *existing = device;
        } else {
            devices.push(device);
        }
        save_devices(&devices_file(&app), &devices);
    }
}

/// Remove the device with the given `ip` and persist. Returns whether one was removed.
#[tauri::command]
fn delete_device(app: tauri::AppHandle, state: tauri::State<'_, DeviceStore>, ip: String) -> bool {
    let mut devices = state.lock().unwrap();
    let before = devices.len();
    devices.retain(|d| d.ip != ip);
    let removed = devices.len() != before;
    if removed {
        save_devices(&devices_file(&app), &devices);
    }
    removed
}

/// Rename the device with the given `ip` and persist.
#[tauri::command]
fn rename_device(
    app: tauri::AppHandle,
    state: tauri::State<'_, DeviceStore>,
    ip: String,
    name: String,
) -> Result<(), String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("Name cannot be empty.".into());
    }
    let mut devices = state.lock().unwrap();
    let device = devices
        .iter_mut()
        .find(|d| d.ip == ip)
        .ok_or("Device not found.")?;
    device.name = trimmed.to_string();
    save_devices(&devices_file(&app), &devices);
    Ok(())
}

/// Run an ONVIF WS-Discovery sweep on a blocking task (it's a sync UDP collect).
#[tauri::command]
async fn discover_devices() -> Vec<DiscoveredDevice> {
    log::info!("discover_devices: starting ONVIF WS-Discovery…");
    let found = tauri::async_runtime::spawn_blocking(onvif::discovery::discover)
        .await
        .unwrap_or_default();
    log::info!("discover_devices: found {} device(s)", found.len());
    found
}

/// Capture a JPEG snapshot from the saved device with `ip`, returned as a
/// `data:image/jpeg;base64,…` URL ready for an `<img src>`.
#[tauri::command]
async fn capture_snapshot(
    state: tauri::State<'_, DeviceStore>,
    ip: String,
) -> Result<String, String> {
    let device = state
        .lock()
        .unwrap()
        .iter()
        .find(|d| d.ip == ip)
        .cloned()
        .ok_or("Device not found.")?;

    log::info!(
        "capture_snapshot: ip={ip} name='{}' xaddr={} has_creds={} rtsp_url={:?}",
        device.name,
        device.xaddr,
        device.has_credentials(),
        device.rtsp_url
    );

    let result = tauri::async_runtime::spawn_blocking(move || capture_snapshot_bytes(&device))
        .await
        .map_err(|e| format!("Snapshot task failed: {e}"))?;

    match &result {
        Ok(b) => log::info!("capture_snapshot: ok — {} bytes", b.len()),
        Err(e) => log::error!("capture_snapshot failed for ip={ip}: {e}"),
    }
    let bytes = result?;
    Ok(format!("data:image/jpeg;base64,{}", B64.encode(&bytes)))
}

/// Validate ONVIF credentials against the device service at `xaddr_or_ip`. On
/// success returns the resolved RTSP stream URI.
#[tauri::command]
async fn test_credentials(
    xaddr_or_ip: String,
    username: String,
    password: String,
) -> Result<String, String> {
    log::info!("test_credentials: target={xaddr_or_ip} user={username}");
    let result = tauri::async_runtime::spawn_blocking(move || {
        onvif::client::test_credentials(&xaddr_or_ip, &username, &password)
    })
    .await
    .map_err(|e| format!("Credential check task failed: {e}"))?;
    match &result {
        Ok(uri) => log::info!("test_credentials: ok — rtsp {uri}"),
        Err(e) => log::error!("test_credentials failed: {e}"),
    }
    result
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // Rust logs → terminal stdout (visible under `task dev`) at Info+.
        .plugin(
            tauri_plugin_log::Builder::new()
                .target(tauri_plugin_log::Target::new(
                    tauri_plugin_log::TargetKind::Stdout,
                ))
                .level(log::LevelFilter::Info)
                .build(),
        )
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let path = devices_file(&app.handle());
            let store: DeviceStore = Arc::new(Mutex::new(load_devices(&path)));
            log::info!(
                "Sentinel backend started — {} device(s) loaded from {}",
                store.lock().unwrap().len(),
                path.display()
            );
            app.manage(store);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_settings,
            set_settings,
            list_devices,
            save_device,
            delete_device,
            rename_device,
            discover_devices,
            capture_snapshot,
            test_credentials,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
