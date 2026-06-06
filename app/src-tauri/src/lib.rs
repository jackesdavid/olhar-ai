//! Tauri shell — thin `#[tauri::command]` wrappers over `app-core` (Bishop's copied logic).
//!
//! M0 foundation: settings, stored under the OS app-config dir (not the ratatui app's
//! `config.json`).
//!
//! Unit 014 — Devices & discovery: a managed `DeviceStore` loaded at `.setup()` from
//! `<app_data_dir>/devices.json`, persisted on every mutation, plus ONVIF discovery,
//! credential validation, and JPEG snapshot commands. Network calls run on blocking
//! tasks so the UI thread never stalls.

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Child;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use app_core::alerts::{self, load_alerts, Alert, AlertLevel, AlertStore};
use app_core::config::{load_settings, save_settings, Settings};
use app_core::device_actions::capture_snapshot_bytes;
use app_core::devices::{load_devices, save_devices, Device, DeviceStore};
use app_core::map::{load_map as core_load_map, save_map as core_save_map, MapRoom};
use app_core::notification::{
    self, load_targets, NotificationConfig, NotificationStore, NotificationTarget,
};
use app_core::onvif;
use app_core::onvif::client::rtsp_url_with_creds;
use app_core::onvif::DiscoveredDevice;
use app_core::workflows::{
    self, ConcurrencyPolicy, Fire, Graph as WfGraph, NodeKind as WfNodeKind, Trigger,
    WorkflowEngine, Workflows,
};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use serde::{Deserialize, Serialize};
use tauri::Manager;

/// Handle to the running go2rtc sidecar so a restart can kill the old one.
/// Managed Tauri state; `None` until the first spawn.
type Go2rtcHandle = Arc<Mutex<Option<Child>>>;

/// The current pairing poller's stop flag, so a config change can restart it
/// without an app restart. Managed Tauri state.
type PairingHandle = Arc<Mutex<Option<Arc<std::sync::atomic::AtomicBool>>>>;

/// (Re)start the Telegram pairing poller: signal any running one to stop, then
/// start a fresh poller if Telegram is configured with a pairing code.
fn restart_pairing(app: &tauri::AppHandle, store: NotificationStore, handle: &PairingHandle) {
    use std::sync::atomic::{AtomicBool, Ordering};
    use tauri::Emitter;
    if let Some(old) = handle.lock().unwrap().take() {
        old.store(true, Ordering::Relaxed); // tell the previous poller to exit
    }
    match resolve_telegram_config(app) {
        Some(cfg) if cfg.can_pair() => {
            let stop = Arc::new(AtomicBool::new(false));
            *handle.lock().unwrap() = Some(stop.clone());
            // On each new pairing, emit so an open Beacons view refreshes live.
            let emitter = app.clone();
            notification::spawn_pairing(store, cfg, notifications_file(app), stop, move || {
                let _ = emitter.emit("beacons-changed", ());
            });
            log::info!("Pairing poller (re)started.");
        }
        Some(_) => log::info!("Telegram token set but no pairing code — poller not started."),
        None => log::info!("Telegram not configured — poller not started."),
    }
}

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

/// The web app's alert log file, under the platform app-data dir (created if missing).
fn alerts_file(app: &tauri::AppHandle) -> PathBuf {
    let dir = app
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| PathBuf::from("."));
    let _ = std::fs::create_dir_all(&dir);
    dir.join("alerts.json")
}

/// The web app's paired-beacons file, under the platform app-data dir (created if missing).
fn notifications_file(app: &tauri::AppHandle) -> PathBuf {
    let dir = app
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| PathBuf::from("."));
    let _ = std::fs::create_dir_all(&dir);
    dir.join("notifications.json")
}

/// The web app's floor-plan file, under the platform app-data dir (created if missing).
fn map_file(app: &tauri::AppHandle) -> PathBuf {
    let dir = app
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| PathBuf::from("."));
    let _ = std::fs::create_dir_all(&dir);
    dir.join("map.json")
}

/// The web app's Telegram config file, under the platform app-data dir. Holds the
/// bot token + pairing code so the app no longer depends on env vars.
fn telegram_file(app: &tauri::AppHandle) -> PathBuf {
    let dir = app
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| PathBuf::from("."));
    let _ = std::fs::create_dir_all(&dir);
    dir.join("telegram.json")
}

/// Persisted Telegram config (bot token + pairing code). Mirrors the fields of
/// `NotificationConfig` but is the on-disk form the app owns.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct StoredTelegramConfig {
    #[serde(default)]
    bot_token: String,
    #[serde(default)]
    pairing_code: String,
}

/// Load the saved Telegram config from `telegram.json`. Returns `None` if the
/// file is missing/empty.
fn load_stored_telegram(app: &tauri::AppHandle) -> Option<StoredTelegramConfig> {
    let raw = std::fs::read_to_string(telegram_file(app)).ok()?;
    serde_json::from_str::<StoredTelegramConfig>(&raw).ok()
}

/// Resolve the active `NotificationConfig`: the saved `telegram.json` if it has a
/// non-empty token, else the environment (`TELEGRAM_BOT_TOKEN` / `…_PAIRING_CODE`).
/// Returns `None` when neither source provides a token.
fn resolve_telegram_config(app: &tauri::AppHandle) -> Option<NotificationConfig> {
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

// ---- Alerts ----------------------------------------------------------------

/// Aggregate counts for the alerts dashboard.
#[derive(Debug, Clone, Serialize)]
struct AlertStats {
    /// Total alerts in the log.
    total: u32,
    /// Alerts whose `created_at` falls on today's local date.
    today: u32,
    /// `(level label, count)` for every level in [`AlertLevel::ALL`] order.
    by_level: Vec<(String, u32)>,
    /// Alerts that have never been delivered (empty `deliveries`).
    undelivered: u32,
}

/// The alert log, newest first.
#[tauri::command]
fn list_alerts(state: tauri::State<'_, AlertStore>) -> Vec<Alert> {
    let mut alerts = state.lock().unwrap().clone();
    alerts.sort_by(|a, b| b.created_at.cmp(&a.created_at).then(b.id.cmp(&a.id)));
    alerts
}

/// Create an alert (level defaults to Info, source defaults to "operator"),
/// persist, and return it. `room` is free text stored as `room_name`; map
/// lookup (`room_id`) lands in a later unit.
#[tauri::command]
fn create_alert(
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
    let room_name = room
        .map(|r| r.trim().to_string())
        .filter(|r| !r.is_empty());

    let alert = alerts::create(
        &state,
        &alerts_file(&app),
        level,
        message,
        None,
        room_name,
        source,
    );
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
fn delete_alert(app: tauri::AppHandle, state: tauri::State<'_, AlertStore>, id: u64) -> bool {
    let removed = alerts::delete(&state, &alerts_file(&app), id);
    log::info!("delete_alert: id={id} removed={removed}");
    removed
}

/// Aggregate counts over the alert log for the dashboard stat cards.
#[tauri::command]
fn alert_stats(state: tauri::State<'_, AlertStore>) -> AlertStats {
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

// ---- Telegram config + Beacons ---------------------------------------------

/// What the UI needs to know about Telegram config, WITHOUT leaking the token:
/// whether a token is set and the pairing code to show the operator.
#[derive(Debug, Clone, Serialize)]
struct TelegramConfigState {
    /// True when a bot token is set (saved file or env).
    configured: bool,
    /// The pairing code users DM the bot (`/pair <code>`); empty when unset.
    pairing_code: String,
}

/// Report whether Telegram is configured + the pairing code (never the token).
#[tauri::command]
fn get_telegram_config(app: tauri::AppHandle) -> TelegramConfigState {
    match resolve_telegram_config(&app) {
        Some(cfg) => TelegramConfigState {
            configured: !cfg.bot_token.trim().is_empty(),
            pairing_code: cfg.pairing_code,
        },
        None => TelegramConfigState { configured: false, pairing_code: String::new() },
    }
}

/// Persist the Telegram bot token + pairing code to `telegram.json`. A token
/// change needs an app restart to (re)start the pairing poller — noted to the
/// caller via the returned string.
#[tauri::command]
fn set_telegram_config(
    app: tauri::AppHandle,
    notif: tauri::State<'_, NotificationStore>,
    pairing: tauri::State<'_, PairingHandle>,
    bot_token: String,
    pairing_code: String,
) -> Result<String, String> {
    // The token is never sent back to the UI, so a blank field means "keep the
    // saved one" — preserve it instead of wiping it.
    let token = {
        let t = bot_token.trim();
        if t.is_empty() {
            load_stored_telegram(&app)
                .map(|s| s.bot_token)
                .unwrap_or_default()
        } else {
            t.to_string()
        }
    };
    let cfg = StoredTelegramConfig {
        bot_token: token,
        pairing_code: pairing_code.trim().to_string(),
    };
    let json = serde_json::to_string_pretty(&cfg).map_err(|e| format!("serialize failed: {e}"))?;
    std::fs::write(telegram_file(&app), json).map_err(|e| format!("write failed: {e}"))?;
    log::info!(
        "set_telegram_config: token={} pairing_code={}",
        if cfg.bot_token.is_empty() { "unset" } else { "set" },
        if cfg.pairing_code.is_empty() { "unset" } else { "set" }
    );
    // Restart the pairing listener with the new config — no app restart needed.
    restart_pairing(&app, notif.inner().clone(), &pairing);
    Ok("Saved — pairing listener restarted.".into())
}

/// The paired beacons (Telegram targets).
#[tauri::command]
fn list_beacons(state: tauri::State<'_, NotificationStore>) -> Vec<NotificationTarget> {
    state.lock().unwrap().clone()
}

/// Enable/disable a beacon by `chat_id` and persist.
#[tauri::command]
fn toggle_beacon(
    app: tauri::AppHandle,
    state: tauri::State<'_, NotificationStore>,
    chat_id: i64,
    enabled: bool,
) -> Result<(), String> {
    let (label, _) = notification::set_enabled_by_id(&state, &notifications_file(&app), chat_id, enabled)
        .ok_or("Beacon not found.")?;
    log::info!("toggle_beacon: '{label}' (chat_id={chat_id}) enabled={enabled}");
    Ok(())
}

/// Rename a beacon by `chat_id` and persist.
#[tauri::command]
fn rename_beacon(
    app: tauri::AppHandle,
    state: tauri::State<'_, NotificationStore>,
    chat_id: i64,
    label: String,
) -> Result<(), String> {
    let new_label = notification::rename_target(&state, &notifications_file(&app), chat_id, &label)?;
    log::info!("rename_beacon: chat_id={chat_id} -> '{new_label}'");
    Ok(())
}

/// Remove a beacon by `chat_id` and persist.
#[tauri::command]
fn delete_beacon(
    app: tauri::AppHandle,
    state: tauri::State<'_, NotificationStore>,
    chat_id: i64,
) -> Result<(), String> {
    let removed = notification::delete_target(&state, &notifications_file(&app), chat_id)
        .ok_or("Beacon not found.")?;
    log::info!("delete_beacon: removed '{removed}' (chat_id={chat_id})");
    Ok(())
}

/// Send a test message to the beacon with `label`. Network IO on a blocking task.
#[tauri::command]
async fn test_beacon(
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

/// Deliver the alert with `id` to every enabled beacon and record the result.
/// Network IO on a blocking task.
#[tauri::command]
async fn deliver_alert(
    app: tauri::AppHandle,
    alert_state: tauri::State<'_, AlertStore>,
    notif_state: tauri::State<'_, NotificationStore>,
    id: u64,
) -> Result<String, String> {
    let cfg = resolve_telegram_config(&app)
        .ok_or("Telegram is not configured (set the bot token in Beacons).")?;
    let alert_store = alert_state.inner().clone();
    let notif_store = notif_state.inner().clone();
    let path = alerts_file(&app);
    let result = tauri::async_runtime::spawn_blocking(move || {
        alerts::deliver(&alert_store, &path, &notif_store, &cfg, id)
    })
    .await
    .map_err(|e| format!("Deliver task failed: {e}"))?;
    match &result {
        Ok(s) => log::info!("deliver_alert: {s}"),
        Err(e) => log::error!("deliver_alert failed for id={id}: {e}"),
    }
    result
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

/// Best-effort, **idempotent** (re)start of the bundled `binaries/go2rtc`
/// (relative to the src-tauri cwd) for the desired `yaml` config.
///
/// Writes the config and (re)starts the sidecar **only when something actually
/// changed** — the on-disk config differs from `yaml`, or the prior child has
/// exited. If go2rtc is already running and serving the same streams, this is a
/// no-op: re-entering the Live page must NOT kill a healthy sidecar out from
/// under a connecting player (that left the iframe on a black screen).
///
/// If the binary is missing (it is never committed) it logs a warning and
/// returns without failing — the rest of the app keeps working.
fn spawn_go2rtc(app: &tauri::AppHandle, config: &std::path::Path, yaml: &str) {
    let handle = app.state::<Go2rtcHandle>();
    let mut guard = handle.lock().unwrap();

    // Is the previously-spawned child still alive?
    let alive = guard
        .as_mut()
        .map(|c| matches!(c.try_wait(), Ok(None)))
        .unwrap_or(false);
    // Does the on-disk config already match what we want to serve?
    let unchanged = std::fs::read_to_string(config).map(|prev| prev == yaml).unwrap_or(false);

    if alive && unchanged {
        return; // already serving the right streams — leave the live sidecar untouched
    }

    if let Err(e) = std::fs::write(config, yaml) {
        log::error!("failed to write go2rtc config: {e}");
        return;
    }

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
    /// go2rtc's ready-made auto-negotiating player URL for this stream
    /// (`stream.html`: WebRTC → MSE → MJPEG fallback; MSE plays in WKWebView).
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
    // Idempotent: only (re)starts go2rtc if the stream set actually changed, so
    // re-entering Live doesn't kill a healthy sidecar mid-connection.
    spawn_go2rtc(&app, &config, &yaml);

    let streams: Vec<StreamInfo> = devices
        .iter()
        .filter(|d| d.rtsp_url.as_deref().map(|u| !u.trim().is_empty()).unwrap_or(false))
        .map(|d| {
            let name = format!("cam_{}", sanitize_ip(&d.ip));
            StreamInfo {
                ip: d.ip.clone(),
                player_url: format!("http://localhost:1984/stream.html?src={name}"),
                name,
            }
        })
        .collect();

    log::info!("ensure_streams: {} stream(s) configured", streams.len());
    Ok(streams)
}

// ---- Map (3D floor plan) ---------------------------------------------------

/// The saved floor plan (rooms + assigned cameras). Stateless: the frontend owns
/// the working copy and saves the whole array back.
#[tauri::command]
fn load_map(app: tauri::AppHandle) -> Vec<MapRoom> {
    core_load_map(&map_file(&app))
}

/// Persist the whole floor-plan array to `<app_data_dir>/map.json`.
#[tauri::command]
fn save_map(app: tauri::AppHandle, rooms: Vec<MapRoom>) {
    core_save_map(&map_file(&app), &rooms);
    log::info!("save_map: persisted {} room(s)", rooms.len());
}

// ---- Tool runtime (shared by chat + workflows) -----------------------------

/// Build a web `ToolContext` whose UI-effect channel is drained to Tauri events
/// on a background thread (the thread ends when the returned context is dropped,
/// closing the Sender). Shared by the chat agent and the workflows engine so tool
/// side-effects — snapshots, store changes, open-live — surface identically.
fn build_run_context(app: &tauri::AppHandle) -> app_core::agent::ToolContext {
    use app_core::agent::ToolUiEvent;
    use tauri::Emitter;

    let devices = app.state::<DeviceStore>().inner().clone();
    let alerts = app.state::<AlertStore>().inner().clone();
    let notif = app.state::<NotificationStore>().inner().clone();

    let (tx, rx) = std::sync::mpsc::channel::<ToolUiEvent>();
    let emit_app = app.clone();
    std::thread::spawn(move || {
        while let Ok(ev) = rx.recv() {
            match ev {
                ToolUiEvent::Snapshot { camera, jpeg } => {
                    let data = B64.encode(&jpeg);
                    let _ = emit_app
                        .emit("chat-image", serde_json::json!({ "camera": camera, "data": data }));
                }
                ToolUiEvent::DevicesChanged => {
                    let _ = emit_app.emit("devices-changed", ());
                }
                ToolUiEvent::AlertsChanged => {
                    let _ = emit_app.emit("alerts-changed", ());
                }
                ToolUiEvent::OpenLive { ip } => {
                    let _ = emit_app.emit("chat-open-live", ip);
                }
            }
        }
    });

    app_core::agent::ToolContext {
        devices,
        devices_path: devices_file(app),
        alerts,
        alerts_path: alerts_file(app),
        notif,
        map_path: map_file(app),
        notif_cfg: resolve_telegram_config(app),
        ui_tx: tx,
    }
}

// ---- Chat / Bishop (agent) -------------------------------------------------

/// The chat conversation history (with verbatim `tool_use`/`tool_result`
/// blocks), held in managed state for a single ongoing conversation.
type ChatHistory = Arc<Mutex<Vec<app_core::agent::Message>>>;

/// Whether the agent is configured (an `ANTHROPIC_API_KEY` is set). The chat
/// screen shows an offline notice when this is false.
#[tauri::command]
fn chat_ready() -> bool {
    app_core::agent::AgentConfig::from_env().is_some()
}

/// Clear the conversation history.
#[tauri::command]
fn reset_chat(history: tauri::State<'_, ChatHistory>) {
    history.lock().unwrap().clear();
}

/// Run one chat turn: append the operator's message, run Bishop's tool-use loop
/// on a blocking task, splice the produced turns into history, and return the
/// final reply. Tool side-effects (snapshots, store changes, open-live) stream
/// to the UI as Tauri events drained off the tool channel while the turn runs.
#[tauri::command]
async fn send_chat(
    app: tauri::AppHandle,
    history_state: tauri::State<'_, ChatHistory>,
    text: String,
) -> Result<String, String> {
    use app_core::agent::{client, Message, ToolRegistry};

    let text = text.trim().to_string();
    if text.is_empty() {
        return Err("Say something first.".into());
    }
    if app_core::agent::AgentConfig::from_env().is_none() {
        return Err("Bishop is offline — set ANTHROPIC_API_KEY to enable chat.".into());
    }

    // Optimistically append the user turn and snapshot the full history for the call.
    let snapshot = {
        let mut h = history_state.lock().unwrap();
        h.push(Message::user(text));
        h.clone()
    };

    // Shared tool context: side-effects stream to the UI while the turn runs.
    let ctx = build_run_context(&app);

    let outcome = tauri::async_runtime::spawn_blocking(move || {
        let registry = ToolRegistry::new();
        client::ask(&snapshot, &registry, &ctx, false)
    })
    .await
    .map_err(|e| format!("Chat task failed: {e}"))?;

    match outcome {
        Some(o) => {
            history_state.lock().unwrap().extend(o.appended);
            Ok(o.reply_text)
        }
        None => {
            // The request failed — roll back the optimistic user turn so history
            // stays valid (no two user turns in a row on the next request).
            history_state.lock().unwrap().pop();
            Err("Bishop didn't respond just now — check the API key and network, then retry.".into())
        }
    }
}

// ---- Workflows (graph automation) ------------------------------------------

/// The named-graph collection, managed state. All graph edits go through commands
/// so Rust stays authoritative for ids + typed-edge validation.
type WfStore = Arc<Mutex<Workflows>>;
/// The trigger/concurrency engine (timers + in-flight counters + alert watermark).
type WfEngine = Arc<Mutex<WorkflowEngine>>;

/// Live run activity per graph, polled by the editor/list: how many runs are in
/// flight and which node is currently executing.
#[derive(Default, Clone, Serialize)]
struct GraphActivity {
    running: u32,
    current: Option<u32>,
}
type WfActivity = Arc<Mutex<HashMap<u32, GraphActivity>>>;

/// A safety backstop: never let more than this many workflow runs be in flight
/// across all graphs at once.
const WF_GLOBAL_CAP: u32 = 8;

fn workflows_file(app: &tauri::AppHandle) -> PathBuf {
    app.path()
        .app_data_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("workflows.json")
}

/// Persist the whole graph collection to `<app_data_dir>/workflows.json`.
fn persist_workflows(app: &tauri::AppHandle, store: &WfStore) {
    let graphs = store.lock().unwrap().graphs.clone();
    workflows::save_workflows(&workflows_file(app), &graphs);
}

/// Spawn one graph run on a background thread. The caller must have already
/// reserved a slot (`engine.on_spawn`). On finish it releases the slot and, if a
/// queued fire is waiting (`QueueSingle`), reserves + spawns the next one.
fn spawn_graph_run(app: &tauri::AppHandle, graph_id: u32) {
    let graph: WfGraph = {
        let store = app.state::<WfStore>();
        let s = store.lock().unwrap();
        match s.graph(graph_id) {
            Some(g) => g.clone(),
            None => return,
        }
    };
    let app = app.clone();
    std::thread::spawn(move || {
        use tauri::Emitter;
        // Mark this graph as running for the live editor/list.
        app.state::<WfActivity>().lock().unwrap().entry(graph_id).or_default().running += 1;

        let result = {
            let ctx = build_run_context(&app);
            let progress_app = app.clone();
            let mut on_node = |nid: u32| {
                progress_app
                    .state::<WfActivity>()
                    .lock()
                    .unwrap()
                    .entry(graph_id)
                    .or_default()
                    .current = Some(nid);
                // Per-node event so the editor highlights every node, even on a
                // run faster than the poll interval.
                let _ = progress_app
                    .emit("workflow-node", serde_json::json!({ "graph": graph_id, "node": nid }));
            };
            workflows::run_dag_with(&graph, &ctx, &mut on_node)
            // ctx drops here → its drain thread ends.
        };
        let (ok, summary) = match &result {
            Ok(outs) => (true, format!("ran {} node(s)", outs.len())),
            Err(e) => (false, e.clone()),
        };

        // Release activity: decrement running, clear the current node when idle.
        {
            let activity = app.state::<WfActivity>();
            let mut a = activity.lock().unwrap();
            let e = a.entry(graph_id).or_default();
            if e.running > 0 {
                e.running -= 1;
            }
            if e.running == 0 {
                e.current = None;
            }
        }
        if ok {
            log::info!("workflow '{}' ({graph_id}) ok: {summary}", graph.name);
        } else {
            log::warn!("workflow '{}' ({graph_id}) failed: {summary}", graph.name);
        }
        let _ = app.emit(
            "workflow-run",
            serde_json::json!({ "graph": graph_id, "name": graph.name, "ok": ok, "summary": summary }),
        );

        // Release the slot; drain a queued fire if the policy buffered one.
        let drain = app.state::<WfEngine>().lock().unwrap().on_finish(graph_id);
        if drain {
            app.state::<WfEngine>().lock().unwrap().on_spawn(graph_id);
            spawn_graph_run(&app, graph_id);
        }
    });
}

/// Apply a graph's concurrency policy to a fire: spawn, queue, or skip.
fn fire_graph(app: &tauri::AppHandle, graph_id: u32, policy: &ConcurrencyPolicy) {
    let decision = app.state::<WfEngine>().lock().unwrap().decide(graph_id, policy);
    match decision {
        Fire::Spawn => {
            app.state::<WfEngine>().lock().unwrap().on_spawn(graph_id);
            spawn_graph_run(app, graph_id);
        }
        Fire::Queue => app.state::<WfEngine>().lock().unwrap().on_queue(graph_id),
        Fire::Skip => {}
    }
}

/// The engine poller: ~1 s tick that fires due Cron graphs and qualifying
/// `OnNewAlert` graphs, each under its concurrency policy. Started at setup.
fn spawn_workflow_poller(app: &tauri::AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(1));
        let now = Instant::now();

        // Snapshot the enabled graphs' firing properties (don't hold the lock during runs).
        let graphs: Vec<(u32, Trigger, ConcurrencyPolicy)> = {
            let s = app.state::<WfStore>();
            let s = s.lock().unwrap();
            s.graphs
                .iter()
                .filter(|g| g.enabled)
                .map(|g| (g.id, g.trigger.clone(), g.concurrency.clone()))
                .collect()
        };

        // Drain new-alert levels ONCE per tick (advances the high-water mark so
        // the backlog fires only once and workflow-made alerts can't loop).
        let levels = {
            let alerts = app.state::<AlertStore>().lock().unwrap().clone();
            app.state::<WfEngine>().lock().unwrap().drain_new_alert_levels(&alerts)
        };

        for (id, trigger, policy) in graphs {
            if app.state::<WfEngine>().lock().unwrap().total_running() >= WF_GLOBAL_CAP {
                break; // global backstop
            }
            let due = match &trigger {
                Trigger::Manual => false,
                Trigger::Cron { every_secs } => {
                    app.state::<WfEngine>().lock().unwrap().cron_due(id, *every_secs, now)
                }
                Trigger::OnNewAlert { min_level } => {
                    WorkflowEngine::levels_qualify(&levels, *min_level)
                }
            };
            if due {
                fire_graph(&app, id, &policy);
            }
        }
    });
}

/// All named graphs (with triggers, concurrency, nodes, typed edges).
#[tauri::command]
fn load_graphs(store: tauri::State<'_, WfStore>) -> Vec<WfGraph> {
    store.lock().unwrap().graphs.clone()
}

/// Live run state per graph id (`{ running, current }`) — the editor/list poll
/// this to show running/idle status and highlight the executing node.
#[tauri::command]
fn run_states(activity: tauri::State<'_, WfActivity>) -> HashMap<u32, GraphActivity> {
    activity.lock().unwrap().clone()
}

/// Static node metadata (kinds + typed ports + default config) for the palette
/// and the editor's client-side type hints. The backend stays authoritative on
/// `add_edge`; this just lets the UI render and pre-filter.
#[tauri::command]
fn node_catalog() -> serde_json::Value {
    let ports = |list: &[workflows::Port]| {
        list.iter()
            .map(|p| {
                serde_json::json!({
                    "name": p.name,
                    "type": serde_json::to_value(p.ty).unwrap_or(serde_json::Value::Null),
                    "required": p.required,
                })
            })
            .collect::<Vec<_>>()
    };
    let kinds = WfNodeKind::ALL
        .iter()
        .map(|k| {
            serde_json::json!({
                "slug": k.slug(),
                "label": k.label(),
                "inputs": ports(k.input_ports()),
                "outputs": ports(k.output_ports()),
                "default_config": k.default_config(),
            })
        })
        .collect::<Vec<_>>();
    serde_json::json!({ "kinds": kinds })
}

#[tauri::command]
fn create_graph(app: tauri::AppHandle, store: tauri::State<'_, WfStore>, name: String) -> u32 {
    let name = if name.trim().is_empty() { "Untitled".to_string() } else { name.trim().to_string() };
    let id = store.lock().unwrap().create_graph(&name);
    persist_workflows(&app, &store);
    id
}

#[tauri::command]
fn rename_graph(app: tauri::AppHandle, store: tauri::State<'_, WfStore>, id: u32, name: String) -> Result<(), String> {
    {
        let mut s = store.lock().unwrap();
        let g = s.graph_mut(id).ok_or("graph not found")?;
        g.name = name.trim().to_string();
    }
    persist_workflows(&app, &store);
    Ok(())
}

#[tauri::command]
fn delete_graph(app: tauri::AppHandle, store: tauri::State<'_, WfStore>, id: u32) {
    store.lock().unwrap().remove_graph(id);
    persist_workflows(&app, &store);
}

#[tauri::command]
fn set_graph_trigger(
    app: tauri::AppHandle,
    store: tauri::State<'_, WfStore>,
    id: u32,
    kind: String,
    every_secs: Option<u64>,
    min_level: Option<String>,
) -> Result<(), String> {
    let trigger = match kind.as_str() {
        "manual" => Trigger::Manual,
        "cron" => Trigger::Cron { every_secs: every_secs.unwrap_or(60).max(1) },
        "on_new_alert" => Trigger::OnNewAlert {
            min_level: min_level
                .as_deref()
                .and_then(AlertLevel::parse)
                .unwrap_or(AlertLevel::Info),
        },
        other => return Err(format!("unknown trigger kind '{other}'")),
    };
    {
        let mut s = store.lock().unwrap();
        s.graph_mut(id).ok_or("graph not found")?.trigger = trigger;
    }
    persist_workflows(&app, &store);
    Ok(())
}

#[tauri::command]
fn set_graph_concurrency(
    app: tauri::AppHandle,
    store: tauri::State<'_, WfStore>,
    id: u32,
    kind: String,
    max: Option<u32>,
) -> Result<(), String> {
    let policy = match kind.as_str() {
        "skip" => ConcurrencyPolicy::SkipIfRunning,
        "queue" => ConcurrencyPolicy::QueueSingle { max_queued: max.unwrap_or(1).max(1) },
        "concurrent" => ConcurrencyPolicy::Concurrent { max_concurrent: max.unwrap_or(2).max(1) },
        other => return Err(format!("unknown concurrency kind '{other}'")),
    };
    {
        let mut s = store.lock().unwrap();
        s.graph_mut(id).ok_or("graph not found")?.concurrency = policy;
    }
    persist_workflows(&app, &store);
    Ok(())
}

#[tauri::command]
fn set_graph_enabled(app: tauri::AppHandle, store: tauri::State<'_, WfStore>, id: u32, enabled: bool) -> Result<(), String> {
    {
        let mut s = store.lock().unwrap();
        s.graph_mut(id).ok_or("graph not found")?.enabled = enabled;
    }
    persist_workflows(&app, &store);
    Ok(())
}

#[tauri::command]
fn add_node(
    app: tauri::AppHandle,
    store: tauri::State<'_, WfStore>,
    graph_id: u32,
    kind: String,
    x: i32,
    y: i32,
) -> Result<u32, String> {
    let kind = WfNodeKind::from_slug(&kind).ok_or_else(|| format!("unknown node kind '{kind}'"))?;
    let id = {
        let mut s = store.lock().unwrap();
        let g = s.graph_mut(graph_id).ok_or("graph not found")?;
        g.add_node(kind, x, y)
    };
    persist_workflows(&app, &store);
    Ok(id)
}

#[tauri::command]
fn remove_node(app: tauri::AppHandle, store: tauri::State<'_, WfStore>, graph_id: u32, node_id: u32) -> Result<(), String> {
    {
        let mut s = store.lock().unwrap();
        s.graph_mut(graph_id).ok_or("graph not found")?.remove_node(node_id);
    }
    persist_workflows(&app, &store);
    Ok(())
}

#[tauri::command]
fn move_node(
    app: tauri::AppHandle,
    store: tauri::State<'_, WfStore>,
    graph_id: u32,
    node_id: u32,
    x: i32,
    y: i32,
) -> Result<(), String> {
    {
        let mut s = store.lock().unwrap();
        let g = s.graph_mut(graph_id).ok_or("graph not found")?;
        let n = g.node_mut(node_id).ok_or("node not found")?;
        n.x = x;
        n.y = y;
    }
    persist_workflows(&app, &store);
    Ok(())
}

#[tauri::command]
fn set_node_config(
    app: tauri::AppHandle,
    store: tauri::State<'_, WfStore>,
    graph_id: u32,
    node_id: u32,
    config: serde_json::Value,
) -> Result<(), String> {
    {
        let mut s = store.lock().unwrap();
        let g = s.graph_mut(graph_id).ok_or("graph not found")?;
        g.node_mut(node_id).ok_or("node not found")?.config = config;
    }
    persist_workflows(&app, &store);
    Ok(())
}

#[tauri::command]
fn add_edge(
    app: tauri::AppHandle,
    store: tauri::State<'_, WfStore>,
    graph_id: u32,
    from: u32,
    from_port: String,
    to: u32,
    to_port: String,
) -> Result<(), String> {
    {
        let mut s = store.lock().unwrap();
        let g = s.graph_mut(graph_id).ok_or("graph not found")?;
        g.add_edge(from, &from_port, to, &to_port)?; // returns the typed-edge validation error
    }
    persist_workflows(&app, &store);
    Ok(())
}

#[tauri::command]
fn remove_edge(app: tauri::AppHandle, store: tauri::State<'_, WfStore>, graph_id: u32, idx: usize) -> Result<(), String> {
    {
        let mut s = store.lock().unwrap();
        s.graph_mut(graph_id).ok_or("graph not found")?.remove_edge(idx);
    }
    persist_workflows(&app, &store);
    Ok(())
}

/// Run-now: fire the graph immediately, bypassing `enabled`/trigger but still
/// respecting its concurrency policy and the global cap.
#[tauri::command]
fn run_graph(app: tauri::AppHandle, store: tauri::State<'_, WfStore>, engine: tauri::State<'_, WfEngine>, id: u32) -> Result<String, String> {
    let policy = {
        let s = store.lock().unwrap();
        s.graph(id).ok_or("graph not found")?.concurrency.clone()
    };
    if engine.lock().unwrap().total_running() >= WF_GLOBAL_CAP {
        return Err("Too many workflows are running right now — try again shortly.".into());
    }
    let decision = engine.lock().unwrap().decide(id, &policy);
    match decision {
        Fire::Spawn => {
            engine.lock().unwrap().on_spawn(id);
            spawn_graph_run(&app, id);
            Ok("Run started.".into())
        }
        Fire::Queue => {
            engine.lock().unwrap().on_queue(id);
            Ok("Queued — a run is already in progress.".into())
        }
        Fire::Skip => Err("Skipped — a run is already in progress (concurrency: skip).".into()),
    }
}

// ---- Voice (TTS) + Speech-to-Text ------------------------------------------

/// Whether voice is configured (an `ELEVENLABS_API_KEY` is set). The chat screen
/// hides/disables the mic + speak controls when false.
#[tauri::command]
fn voice_ready() -> bool {
    app_core::voice::VoiceConfig::from_env().is_some()
}

/// Synthesize `text` to speech and return base64 MP3 for the WebView to play.
#[tauri::command]
async fn speak(text: String) -> Result<String, String> {
    let text = text.trim().to_string();
    if text.is_empty() {
        return Err("Nothing to speak.".into());
    }
    let mp3 = tauri::async_runtime::spawn_blocking(move || app_core::voice::synthesize(&text))
        .await
        .map_err(|e| format!("Speak task failed: {e}"))?;
    match mp3 {
        Some(bytes) => Ok(B64.encode(&bytes)),
        None => Err("Voice synthesis failed — check ELEVENLABS_API_KEY and network.".into()),
    }
}

/// A filename for the Scribe multipart part, derived from the recorded MIME.
fn audio_filename_for(mime: &str) -> String {
    let ext = match mime.split(';').next().unwrap_or("").trim() {
        "audio/webm" => "webm",
        "audio/ogg" => "ogg",
        "audio/mp4" | "audio/aac" => "mp4",
        "audio/mpeg" => "mp3",
        "audio/wav" | "audio/x-wav" => "wav",
        _ => "webm",
    };
    format!("speech.{ext}")
}

/// Transcribe a base64-encoded audio clip (recorded in the WebView) via Scribe.
#[tauri::command]
async fn transcribe(audio_b64: String, mime: Option<String>) -> Result<String, String> {
    let bytes = B64.decode(audio_b64.as_bytes()).map_err(|e| format!("Bad audio data: {e}"))?;
    let mime = mime.unwrap_or_else(|| "audio/webm".into());
    let filename = audio_filename_for(&mime);
    let out = tauri::async_runtime::spawn_blocking(move || {
        app_core::listen::transcribe(&bytes, &filename, &mime)
    })
    .await
    .map_err(|e| format!("Transcribe task failed: {e}"))?;
    match &out {
        Ok(t) => log::info!("transcribe: {} chars", t.len()),
        Err(e) => log::warn!("transcribe failed: {e}"),
    }
    out
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

            // Alerts: managed log loaded from <app_data_dir>/alerts.json.
            let alerts_path = alerts_file(&app.handle());
            let alert_store: AlertStore = Arc::new(Mutex::new(load_alerts(&alerts_path)));
            log::info!(
                "Alerts loaded — {} alert(s) from {}",
                alert_store.lock().unwrap().len(),
                alerts_path.display()
            );
            app.manage(alert_store);

            // Beacons: managed target list loaded from <app_data_dir>/notifications.json.
            let notif_path = notifications_file(&app.handle());
            let notif_store: NotificationStore =
                Arc::new(Mutex::new(load_targets(&notif_path)));
            log::info!(
                "Beacons loaded — {} target(s) from {}",
                notif_store.lock().unwrap().len(),
                notif_path.display()
            );
            app.manage(notif_store);

            // Pairing poller: managed stop-handle so `set_telegram_config` can
            // restart the inbound `/pair <code>` listener without an app restart.
            let pairing: PairingHandle = Arc::new(Mutex::new(None));
            app.manage(pairing.clone());
            let store = app.handle().state::<NotificationStore>().inner().clone();
            restart_pairing(&app.handle(), store, &pairing);

            // Live video: managed handle for the go2rtc sidecar, then a
            // best-effort startup spawn so streams are ready on launch (no-op if
            // the binary is absent or no devices have an rtsp_url yet).
            let go2rtc: Go2rtcHandle = Arc::new(Mutex::new(None));
            app.manage(go2rtc);

            // Chat: a single managed conversation history for Bishop's agent.
            let history: ChatHistory = Arc::new(Mutex::new(Vec::new()));
            app.manage(history);

            // Workflows: named graphs + the trigger/concurrency engine (seeded
            // from the current alert max so the backlog doesn't fire OnNewAlert),
            // then the background poller that drives Cron / OnNewAlert.
            let wf_graphs = workflows::load_workflows(&workflows_file(&app.handle()));
            log::info!("Workflows loaded — {} graph(s)", wf_graphs.len());
            let wf_store: WfStore = Arc::new(Mutex::new(Workflows::from_graphs(wf_graphs)));
            app.manage(wf_store);
            let alert_max = app
                .handle()
                .state::<AlertStore>()
                .lock()
                .unwrap()
                .iter()
                .map(|a| a.id)
                .max()
                .unwrap_or(0);
            let wf_engine: WfEngine = Arc::new(Mutex::new(WorkflowEngine::new(alert_max)));
            app.manage(wf_engine);
            let wf_activity: WfActivity = Arc::new(Mutex::new(HashMap::new()));
            app.manage(wf_activity);
            spawn_workflow_poller(&app.handle());
            let handle = app.handle().clone();
            let devices = handle.state::<DeviceStore>().lock().unwrap().clone();
            let config = go2rtc_config_file(&handle);
            spawn_go2rtc(&handle, &config, &build_go2rtc_yaml(&devices));
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
            list_alerts,
            create_alert,
            delete_alert,
            alert_stats,
            get_telegram_config,
            set_telegram_config,
            list_beacons,
            toggle_beacon,
            rename_beacon,
            delete_beacon,
            test_beacon,
            deliver_alert,
            load_map,
            save_map,
            chat_ready,
            reset_chat,
            send_chat,
            load_graphs,
            run_states,
            node_catalog,
            create_graph,
            rename_graph,
            delete_graph,
            set_graph_trigger,
            set_graph_concurrency,
            set_graph_enabled,
            add_node,
            remove_node,
            move_node,
            set_node_config,
            add_edge,
            remove_edge,
            run_graph,
            voice_ready,
            speak,
            transcribe,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
