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
use std::process::Child;
use std::sync::{Arc, Mutex};

use app_core::config::{load_settings, save_settings, Settings};
use app_core::device_actions::capture_snapshot_bytes;
use app_core::devices::{load_devices, save_devices, Device, DeviceStore};
use app_core::onvif;
use app_core::onvif::client::rtsp_url_with_creds;
use app_core::onvif::DiscoveredDevice;
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use serde::Serialize;
use tauri::Manager;

/// Handle to the running go2rtc sidecar so a restart can kill the old one.
/// Managed Tauri state; `None` until the first spawn.
type Go2rtcHandle = Arc<Mutex<Option<Child>>>;

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
    app: tauri::AppHandle,
    state: tauri::State<'_, DeviceStore>,
    ip: String,
) -> Result<String, String> {
    let mut device = state
        .lock()
        .unwrap()
        .iter()
        .find(|d| d.ip == ip)
        .cloned()
        .ok_or("Device not found.")?;

    // Self-heal: many cameras have no ONVIF GetSnapshotUri and need the RTSP fallback,
    // which requires `rtsp_url`. If it's missing but we have creds, resolve it via
    // ONVIF (GetStreamUri) and persist it onto the device — so snapshots + live "just work".
    let needs_rtsp = device.rtsp_url.as_deref().unwrap_or("").trim().is_empty();
    if needs_rtsp && device.has_credentials() {
        log::info!("capture_snapshot: rtsp_url missing — resolving via ONVIF for ip={ip}");
        let xaddr = if device.xaddr.is_empty() {
            device.ip.clone()
        } else {
            device.xaddr.clone()
        };
        let user = device.username.clone().unwrap_or_default();
        let pass = device.password.clone().unwrap_or_default();
        match tauri::async_runtime::spawn_blocking(move || {
            onvif::client::test_credentials(&xaddr, &user, &pass)
        })
        .await
        .map_err(|e| format!("Resolve task failed: {e}"))?
        {
            Ok(rtsp) => {
                device.rtsp_url = Some(rtsp.clone());
                let mut devices = state.lock().unwrap();
                if let Some(d) = devices.iter_mut().find(|d| d.ip == ip) {
                    d.rtsp_url = Some(rtsp);
                }
                save_devices(&devices_file(&app), &devices);
                log::info!("capture_snapshot: resolved + persisted rtsp_url for ip={ip}");
            }
            Err(e) => log::warn!("capture_snapshot: could not resolve rtsp_url for ip={ip}: {e}"),
        }
    }

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

/// Validate a saved device's ONVIF credentials and persist them along with the
/// resolved RTSP URL. Looks the device up by `ip` in the store (to reuse its
/// `xaddr`), runs `test_credentials` on a blocking task, then writes
/// `username`/`password`/`rtsp_url` back to `devices.json`. Returns the updated
/// `Device`. This is what makes both live video and snapshots work (rtsp_url +
/// creds get persisted instead of being lost after a one-shot test).
#[tauri::command]
async fn validate_device(
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
        save_devices(&devices_file(&app), &devices);
        updated
    };

    log::info!("validate_device: ok — ip={ip} rtsp_url={rtsp}");
    Ok(updated)
}

// ---- Live video (go2rtc sidecar) -------------------------------------------

/// The generated go2rtc config, under the platform app-data dir.
fn go2rtc_config_file(app: &tauri::AppHandle) -> PathBuf {
    let dir = app
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| PathBuf::from("."));
    let _ = std::fs::create_dir_all(&dir);
    dir.join("go2rtc.yaml")
}

/// Sanitize an IP into a go2rtc stream-name suffix (dots → underscores).
fn sanitize_ip(ip: &str) -> String {
    ip.replace('.', "_")
}

/// Build a go2rtc YAML from the device store: one `cam_<sanitized_ip>` stream
/// per device that has a non-empty `rtsp_url`, with credentials injected via
/// `rtsp_url_with_creds`. Returns the YAML text (always includes a `streams:`
/// key, even if empty, so go2rtc starts cleanly).
fn build_go2rtc_yaml(devices: &[Device]) -> String {
    let mut out = String::from("# Generated by Sentinel — do not edit by hand.\nstreams:\n");
    for d in devices {
        let Some(rtsp) = d.rtsp_url.as_deref() else {
            continue;
        };
        if rtsp.trim().is_empty() {
            continue;
        }
        let user = d.username.as_deref().unwrap_or("");
        let pass = d.password.as_deref().unwrap_or("");
        let url = rtsp_url_with_creds(rtsp, user, pass);
        out.push_str(&format!("  cam_{}: {}\n", sanitize_ip(&d.ip), url));
    }
    out
}

/// Best-effort spawn of the bundled `binaries/go2rtc` (relative to the src-tauri
/// cwd) pointing at the generated config. Kills any previously-spawned child
/// first. If the binary is missing (it is never committed), logs a warning and
/// returns without failing — the rest of the app keeps working.
fn spawn_go2rtc(app: &tauri::AppHandle, config: &std::path::Path) {
    let handle = app.state::<Go2rtcHandle>();
    let mut guard = handle.lock().unwrap();

    // Kill a prior instance so a restart doesn't leave an orphan on :1984.
    if let Some(mut child) = guard.take() {
        let _ = child.kill();
        let _ = child.wait();
    }

    let bin = if cfg!(windows) {
        std::path::Path::new("binaries/go2rtc.exe")
    } else {
        std::path::Path::new("binaries/go2rtc")
    };
    if !bin.exists() {
        log::warn!(
            "go2rtc not found at {bin:?} — live video disabled until you drop the binary in (see binaries/DOWNLOAD.md)"
        );
        return;
    }

    match std::process::Command::new(bin)
        .args(["-config"])
        .arg(config)
        .spawn()
    {
        Ok(child) => {
            log::info!("go2rtc started (pid {}) → http://localhost:1984", child.id());
            *guard = Some(child);
        }
        Err(e) => log::error!("failed to start go2rtc: {e}"),
    }
}

/// One playable stream for the Live screen.
#[derive(Debug, Clone, Serialize)]
struct StreamInfo {
    ip: String,
    /// go2rtc stream name, e.g. `cam_192_168_1_50`.
    name: String,
    /// go2rtc's ready-made WebRTC player URL for this stream.
    player_url: String,
}

/// Regenerate the go2rtc config from the current device store, (re)start the
/// sidecar, and return one `StreamInfo` per device that has an `rtsp_url`.
#[tauri::command]
async fn ensure_streams(
    app: tauri::AppHandle,
    state: tauri::State<'_, DeviceStore>,
) -> Result<Vec<StreamInfo>, String> {
    let devices = state.lock().unwrap().clone();

    let yaml = build_go2rtc_yaml(&devices);
    let config = go2rtc_config_file(&app);
    std::fs::write(&config, &yaml).map_err(|e| format!("Failed to write go2rtc config: {e}"))?;

    spawn_go2rtc(&app, &config);

    let streams: Vec<StreamInfo> = devices
        .iter()
        .filter(|d| d.rtsp_url.as_deref().map(|u| !u.trim().is_empty()).unwrap_or(false))
        .map(|d| {
            let name = format!("cam_{}", sanitize_ip(&d.ip));
            StreamInfo {
                ip: d.ip.clone(),
                player_url: format!("http://localhost:1984/webrtc.html?src={name}"),
                name,
            }
        })
        .collect();

    log::info!("ensure_streams: {} stream(s) configured", streams.len());
    Ok(streams)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // Rust logs → terminal stdout (visible under `task dev`) at Info+.
        .plugin(
            tauri_plugin_log::Builder::new()
                // clear_targets() first — the default set already includes Stdout, and
                // `.target()` APPENDS, so without this you get two Stdout targets → doubled logs.
                .clear_targets()
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

            // Live video: managed handle for the go2rtc sidecar, then a
            // best-effort startup spawn so streams are ready on launch (no-op if
            // the binary is absent or no devices have an rtsp_url yet).
            let go2rtc: Go2rtcHandle = Arc::new(Mutex::new(None));
            app.manage(go2rtc);
            let handle = app.handle().clone();
            let devices = handle.state::<DeviceStore>().lock().unwrap().clone();
            let config = go2rtc_config_file(&handle);
            if std::fs::write(&config, build_go2rtc_yaml(&devices)).is_ok() {
                spawn_go2rtc(&handle, &config);
            }
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
            validate_device,
            ensure_streams,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
