//! Devices — Tauri adapter for the device registry slice: the saved camera list,
//! ONVIF discovery + credential validation, and the hourly rescan that reconciles
//! IP changes. Live feed + snapshots live in the `stream` slice. The domain lives
//! in `app_core::devices`; persistence in `app_core::devices::repo`; ONVIF in
//! `app_core::onvif`.
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tauri::{Emitter, Manager};

use app_core::alerts::{self, AlertLevel, AlertStore};
use app_core::db::Db;
use app_core::devices::{self, Device, DeviceStore};
use app_core::onvif::{self, DiscoveredDevice};

use crate::db_ref;

// ---- lifecycle (called by lib.rs setup / try_connect_db) --------------------

/// Manage an empty device store BEFORE the DB connect so [`hydrate`] can fill it.
pub fn manage_empty_store(app: &tauri::AppHandle) {
    let store: DeviceStore = Arc::new(Mutex::new(Vec::new()));
    app.manage(store);
}

/// Hydrate the in-memory device store from Postgres.
pub fn hydrate(app: &tauri::AppHandle, db: &Db) {
    let devices = devices::repo::load(db);
    log::info!("Devices loaded from Postgres — {} device(s)", devices.len());
    *app.state::<DeviceStore>().lock().unwrap() = devices;
}

/// Persist the full device list to Postgres. Errors if the DB isn't connected yet
/// (a safety net — commands only run once the app is past the splash). `pub(crate)`
/// so the `stream` slice can self-heal a device's `rtsp_url`.
pub(crate) fn persist_devices(app: &tauri::AppHandle, list: &[Device]) -> Result<(), String> {
    devices::repo::save(&db_ref(app)?, list);
    Ok(())
}

// ---- commands ---------------------------------------------------------------

/// A snapshot of the saved device list.
#[tauri::command]
pub fn list_devices(state: tauri::State<'_, DeviceStore>) -> Vec<Device> {
    state.lock().unwrap().clone()
}

/// Upsert a device by `ip` (replace in place if present, else append) and persist.
#[tauri::command]
pub fn save_device(
    app: tauri::AppHandle,
    state: tauri::State<'_, DeviceStore>,
    device: Device,
) -> Result<(), String> {
    let mut devices = state.lock().unwrap();
    if let Some(existing) = devices.iter_mut().find(|d| d.ip == device.ip) {
        *existing = device;
    } else {
        devices.push(device);
    }
    persist_devices(&app, &devices)
}

/// Remove the device with the given `ip` and persist. Returns whether one was removed.
#[tauri::command]
pub fn delete_device(
    app: tauri::AppHandle,
    state: tauri::State<'_, DeviceStore>,
    ip: String,
) -> Result<bool, String> {
    let mut devices = state.lock().unwrap();
    let before = devices.len();
    devices.retain(|d| d.ip != ip);
    let removed = devices.len() != before;
    if removed {
        persist_devices(&app, &devices)?;
    }
    Ok(removed)
}

/// Rename the device with the given `ip` and persist.
#[tauri::command]
pub fn rename_device(
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
    persist_devices(&app, &devices)?;
    Ok(())
}

/// Run an ONVIF WS-Discovery sweep on a blocking task (it's a sync UDP collect).
#[tauri::command]
pub async fn discover_devices(app: tauri::AppHandle) -> Vec<DiscoveredDevice> {
    log::info!("discover_devices: starting ONVIF WS-Discovery…");
    let found = tauri::async_runtime::spawn_blocking(onvif::discovery::discover)
        .await
        .unwrap_or_default();
    log::info!("discover_devices: found {} device(s)", found.len());
    reconcile_devices(&app, &found);
    found
}

/// Reconcile saved devices against a fresh discovery. Match by the stable ONVIF
/// id; when a camera's IP changed (DHCP / power-cycle) update its ip/xaddr/
/// rtsp_url, persist, and raise a warning alert. Also backfills the stable id on
/// devices saved before it existed (matched by current IP).
fn reconcile_devices(app: &tauri::AppHandle, found: &[DiscoveredDevice]) {
    let store = app.state::<DeviceStore>();
    let mut changed = false;
    let mut ip_changes: Vec<(String, String, String)> = Vec::new(); // (name, old_ip, new_ip)

    {
        let mut devices = store.lock().unwrap();
        for d in found {
            if d.id.is_empty() {
                continue; // can't reconcile a camera that didn't advertise an id
            }
            if let Some(saved) = devices.iter_mut().find(|s| s.id == d.id) {
                if saved.ip != d.ip {
                    let old = std::mem::replace(&mut saved.ip, d.ip.clone());
                    saved.xaddr = d.xaddr.clone();
                    saved.rtsp_url = saved.rtsp_url.clone().map(|u| u.replace(&old, &d.ip));
                    ip_changes.push((saved.name.clone(), old, d.ip.clone()));
                    changed = true;
                }
            } else if let Some(saved) = devices.iter_mut().find(|s| s.id.is_empty() && s.ip == d.ip) {
                saved.id = d.id.clone(); // backfill the stable id on a pre-existing device
                changed = true;
            }
        }
        if changed {
            let _ = persist_devices(app, &devices); // best-effort; rescan retries hourly
        }
    }

    if changed {
        let _ = app.emit("devices-changed", ());
    }
    if !ip_changes.is_empty() {
        let alert_store = app.state::<AlertStore>().inner().clone();
        let db = db_ref(app).ok();
        for (name, old, new) in ip_changes {
            let msg = format!("Camera '{name}' changed IP address: {old} → {new}");
            log::warn!("reconcile: {msg}");
            alerts::create(&alert_store, db.as_ref(), AlertLevel::Warn, msg, None, None, "scanner".into());
        }
        let _ = app.emit("alerts-changed", ());
    }
}

/// Hourly background rescan: re-discover ONVIF cameras and reconcile the store so
/// a camera that changed IP (DHCP / power-cycle) is found again automatically.
pub fn spawn_device_rescan(app: &tauri::AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(3600));
        let found = onvif::discovery::discover();
        if !found.is_empty() {
            reconcile_devices(&app, &found);
        }
    });
}

/// Validate ONVIF credentials against the device service at `xaddr_or_ip`. On
/// success returns the resolved RTSP stream URI.
#[tauri::command]
pub async fn test_credentials(
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

/// Validate a saved device's ONVIF credentials and persist them along with the
/// resolved RTSP URL. Looks the device up by `ip` in the store (to reuse its
/// `xaddr`), runs `test_credentials` on a blocking task, then writes
/// `username`/`password`/`rtsp_url` back to Postgres. Returns the updated
/// `Device`. This is what makes both live video and snapshots work (rtsp_url +
/// creds get persisted instead of being lost after a one-shot test).
#[tauri::command]
pub async fn validate_device(
    app: tauri::AppHandle,
    state: tauri::State<'_, DeviceStore>,
    ip: String,
    username: String,
    password: String,
) -> Result<Device, String> {
    // Resolve the device's xaddr (fall back to its ip) from the store.
    let xaddr = {
        let devices = state.lock().unwrap();
        let device = devices
            .iter()
            .find(|d| d.ip == ip)
            .ok_or("Device not found.")?;
        if device.xaddr.is_empty() {
            device.ip.clone()
        } else {
            device.xaddr.clone()
        }
    };

    log::info!("validate_device: ip={ip} xaddr={xaddr} user={username}");

    let user = username.clone();
    let pass = password.clone();
    let rtsp = tauri::async_runtime::spawn_blocking(move || {
        onvif::client::test_credentials(&xaddr, &user, &pass)
    })
    .await
    .map_err(|e| format!("Credential check task failed: {e}"))?;

    let rtsp = match rtsp {
        Ok(uri) => uri,
        Err(e) => {
            log::error!("validate_device failed for ip={ip}: {e}");
            return Err(e);
        }
    };

    // Persist creds + rtsp_url onto the saved device.
    let updated = {
        let mut devices = state.lock().unwrap();
        let device = devices
            .iter_mut()
            .find(|d| d.ip == ip)
            .ok_or("Device not found.")?;
        device.username = Some(username);
        device.password = Some(password);
        device.rtsp_url = Some(rtsp.clone());
        let updated = device.clone();
        persist_devices(&app, &devices)?;
        updated
    };

    log::info!("validate_device: ok — ip={ip} rtsp_url={rtsp}");
    Ok(updated)
}
