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
use std::time::Duration;

use app_core::alerts::{self, Alert, AlertLevel, AlertStore};
use app_core::config::{load_settings, save_settings, Settings};
use app_core::db::Db;
use app_core::device_actions::capture_snapshot_bytes;
use app_core::devices::{Device, DeviceStore};
use app_core::persons::{self, Person, PersonEmbedding, PersonStore, RecognitionResult};
use app_core::map::{load_map as core_load_map, MapRoom};
use app_core::notification::{
    self, load_targets, NotificationConfig, NotificationStore, NotificationTarget,
};
use app_core::onvif;
use app_core::onvif::client::rtsp_url_with_creds;
use app_core::onvif::DiscoveredDevice;
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use serde::{Deserialize, Serialize};
use tauri::Manager;

/// Per-feature Tauri adapters (commands + pollers + state lifecycle).
mod features;

/// Handle to the running go2rtc sidecar so a restart can kill the old one.
/// Managed Tauri state; `None` until the first spawn.
type Go2rtcHandle = Arc<Mutex<Option<Child>>>;

/// Stop-flag for the keep-warm poller (pings each camera's go2rtc frame so the
/// shared feed stays connected when "always warm" is on). `None` when off.
/// Managed Tauri state.
type WarmHandle = Arc<Mutex<Option<Arc<std::sync::atomic::AtomicBool>>>>;

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
            notification::spawn_pairing(store, cfg, db_ref(app).ok(), stop, move || {
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

/// The Postgres handle (devices store). `None` until the splash's `boot_status`
/// drives a successful connect; managed Tauri state.
type DbState = Arc<Mutex<Option<Db>>>;

/// Connect to Postgres + apply migrations (once), then hydrate the in-memory
/// `DeviceStore` from the DB. Idempotent — a no-op if already connected. The
/// splash retries this until it returns `Ok`, so the window stays on the splash
/// while the database comes up (the app has no JSON fallback).
fn try_connect_db(app: &tauri::AppHandle) -> Result<(), String> {
    let state = app.state::<DbState>();
    if state.lock().unwrap().is_some() {
        return Ok(()); // already connected
    }
    let url = std::env::var("DATABASE_URL").map_err(|_| "DATABASE_URL is not set".to_string())?;
    let db = Db::connect(&url).map_err(|e| e.to_string())?;
    let devices = db.load_devices();
    log::info!(
        "Postgres connected; migrations applied — {} device(s) loaded",
        devices.len()
    );
    *app.state::<DeviceStore>().lock().unwrap() = devices;
    let people = db.load_persons();
    log::info!("Persons loaded — {} person(s)", people.len());
    *app.state::<PersonStore>().lock().unwrap() = people;
    // The alert log lives in Postgres — hydrate the in-memory store. One-time,
    // non-destructive import: if the DB log is empty but a legacy alerts.json exists,
    // seed the DB from it (ids preserved) so the history survives the move to the DB.
    let mut alerts = alerts::repo::load(&db);
    if alerts.is_empty() {
        let from_json = alerts::repo::import_legacy_json(&alerts_file(app));
        if !from_json.is_empty() {
            for a in &from_json {
                alerts::repo::insert(&db, a);
            }
            log::info!("Imported {} alert(s) from alerts.json into Postgres", from_json.len());
            alerts = alerts::repo::load(&db);
        }
    }
    log::info!("Alerts loaded from Postgres — {} alert(s)", alerts.len());
    *app.state::<AlertStore>().lock().unwrap() = alerts;
    // Beacons (Telegram targets) live in Postgres too — same hydrate + one-time
    // non-destructive import from a legacy notifications.json when the DB is empty.
    let mut beacons = db.load_beacons();
    if beacons.is_empty() {
        let from_json = load_targets(&notifications_file(app));
        if !from_json.is_empty() {
            db.save_beacons(&from_json);
            log::info!("Imported {} beacon(s) from notifications.json into Postgres", from_json.len());
            beacons = db.load_beacons();
        }
    }
    log::info!("Beacons loaded from Postgres — {} target(s)", beacons.len());
    *app.state::<NotificationStore>().lock().unwrap() = beacons;
    // Workflow graphs live in Postgres too — the workflows slice owns its hydration.
    features::workflows::hydrate(app, &db);

    // Map rooms + Telegram config also live in Postgres (read on demand, no store).
    // One-time non-destructive import from legacy JSON files when the DB is empty.
    if db.load_map().is_empty() {
        let from_json = core_load_map(&map_file(app));
        if !from_json.is_empty() {
            db.save_map(&from_json);
            log::info!("Imported {} map room(s) from map.json into Postgres", from_json.len());
        }
    }
    if db.load_telegram().is_none() {
        if let Some(stored) = std::fs::read_to_string(telegram_file(app))
            .ok()
            .and_then(|raw| serde_json::from_str::<StoredTelegramConfig>(&raw).ok())
        {
            if !stored.bot_token.trim().is_empty() || !stored.pairing_code.trim().is_empty() {
                db.save_telegram(&stored.bot_token, &stored.pairing_code);
                log::info!("Imported Telegram config from telegram.json into Postgres");
            }
        }
    }

    *state.lock().unwrap() = Some(db);
    Ok(())
}

/// Persist the full device list to Postgres. Errors if the DB isn't connected yet
/// (a safety net — commands only run once the app is past the splash).
fn persist_devices(app: &tauri::AppHandle, devices: &[Device]) -> Result<(), String> {
    app.state::<DbState>()
        .lock()
        .unwrap()
        .as_ref()
        .ok_or("Database not ready")?
        .save_devices(devices);
    Ok(())
}

/// Borrow the connected DB as a cloned handle (cheap — shares pool + runtime), or
/// error if the database isn't ready yet. `pub(crate)` so per-feature adapters
/// (under `features::`) can reach the shared DB.
pub(crate) fn db_ref(app: &tauri::AppHandle) -> Result<Db, String> {
    app.state::<DbState>()
        .lock()
        .unwrap()
        .as_ref()
        .cloned()
        .ok_or_else(|| "Database not ready".to_string())
}

/// Persist the full person list to Postgres (upsert + prune). Errors if the DB
/// isn't connected yet (commands only run once the app is past the splash).
fn persist_persons(app: &tauri::AppHandle, people: &[Person]) -> Result<(), String> {
    app.state::<DbState>()
        .lock()
        .unwrap()
        .as_ref()
        .ok_or("Database not ready")?
        .save_persons(people);
    Ok(())
}

/// Splash boot probe: ensure Postgres is connected + migrated. The splash calls
/// this on mount and retries until it returns `Ok`, then reveals the app.
#[tauri::command]
fn boot_status(app: tauri::AppHandle) -> Result<(), String> {
    try_connect_db(&app)
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

/// Load the saved Telegram config from Postgres. Returns `None` if unset or the DB
/// isn't connected yet.
fn load_stored_telegram(app: &tauri::AppHandle) -> Option<StoredTelegramConfig> {
    let (bot_token, pairing_code) = db_ref(app).ok()?.load_telegram()?;
    Some(StoredTelegramConfig { bot_token, pairing_code })
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
    // React to the "keep camera feeds always on" toggle without an app restart.
    reconcile_keepwarm(&app);
}

// ---- Devices ---------------------------------------------------------------

/// A snapshot of the saved device list.
#[tauri::command]
fn list_devices(state: tauri::State<'_, DeviceStore>) -> Vec<Device> {
    state.lock().unwrap().clone()
}

/// Upsert a device by `ip` (replace in place if present, else append) and persist.
#[tauri::command]
fn save_device(
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
fn delete_device(
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
    persist_devices(&app, &devices)?;
    Ok(())
}

// ---- People ----------------------------------------------------------------

/// The managed person list (the hierarchy + recipient links + permissions).
#[tauri::command]
fn list_people(state: tauri::State<'_, PersonStore>) -> Vec<Person> {
    state.lock().unwrap().clone()
}

/// The permission actions a superior can be granted over its subtree — the single
/// source of truth for the People UI's permission toggles.
#[tauri::command]
fn person_permission_kinds() -> Vec<String> {
    persons::PERMISSIONS.iter().map(|s| s.to_string()).collect()
}

/// Upsert a person by `id` (the frontend assigns a UUID for new people) and
/// persist. Validates the name, the recipient label, and guards the hierarchy
/// against self/cycle parents before saving.
#[tauri::command]
fn save_person(
    app: tauri::AppHandle,
    state: tauri::State<'_, PersonStore>,
    person: Person,
) -> Result<(), String> {
    let mut person = person;
    person.name = person.name.trim().to_string();
    if person.id.trim().is_empty() {
        return Err("Person id is required.".into());
    }
    if person.name.is_empty() {
        return Err("Name cannot be empty.".into());
    }
    // Normalize an empty recipient/parent to None.
    person.recipient_label =
        person.recipient_label.filter(|r| !r.trim().is_empty());
    person.parent_id = person.parent_id.filter(|p| !p.trim().is_empty());
    // Drop unknown permission actions.
    person.permissions.retain(|a| persons::PERMISSIONS.contains(&a.as_str()));

    let mut people = state.lock().unwrap();
    if let Some(parent) = person.parent_id.as_deref() {
        if persons::find(&people, parent).is_none() {
            return Err("Selected parent no longer exists.".into());
        }
        if persons::would_cycle(&people, &person.id, parent) {
            return Err("That parent would create a cycle in the hierarchy.".into());
        }
    }
    if let Some(existing) = people.iter_mut().find(|p| p.id == person.id) {
        *existing = person;
    } else {
        people.push(person);
    }
    persist_persons(&app, &people)
}

/// Remove a person and detach any children that pointed to them (so the in-memory
/// store mirrors the table's `ON DELETE SET NULL`). Returns whether one was removed.
#[tauri::command]
fn delete_person(
    app: tauri::AppHandle,
    state: tauri::State<'_, PersonStore>,
    id: String,
) -> Result<bool, String> {
    let mut people = state.lock().unwrap();
    let before = people.len();
    people.retain(|p| p.id != id);
    let removed = people.len() != before;
    if removed {
        for child in people.iter_mut().filter(|p| p.parent_id.as_deref() == Some(id.as_str())) {
            child.parent_id = None;
        }
        persist_persons(&app, &people)?;
    }
    Ok(removed)
}

/// Enroll one face for a person: compute an embedding from `image` (base64, optionally
/// a `data:…;base64,` URL) and insert it as a new `person_embeddings` row. `source` is
/// `"camera"` or `"upload"`; `approved` carries the per-sample consent toggle. Returns
/// the new embedding's id.
#[tauri::command]
fn enroll_face(
    app: tauri::AppHandle,
    state: tauri::State<'_, PersonStore>,
    person_id: String,
    image: String,
    source: String,
    approved: bool,
) -> Result<String, String> {
    {
        let people = state.lock().unwrap();
        if persons::find(&people, &person_id).is_none() {
            return Err("Person not found.".into());
        }
    }
    let b64 = image.rsplit(',').next().unwrap_or(&image);
    let bytes = B64.decode(b64.trim()).map_err(|e| format!("decode image: {e}"))?;
    let embedding = app_core::agent::face::embed(&bytes)?;
    db_ref(&app)?.insert_embedding(&person_id, &embedding, approved, &source)
}

/// Every enrolled embedding for a person (with the raw vector, for the avatar).
#[tauri::command]
fn list_embeddings(app: tauri::AppHandle, person_id: String) -> Result<Vec<PersonEmbedding>, String> {
    Ok(db_ref(&app)?.list_embeddings(&person_id))
}

/// Approve a single embedding (per-sample consent).
#[tauri::command]
fn approve_embedding(app: tauri::AppHandle, id: String) -> Result<(), String> {
    db_ref(&app)?.approve_embedding(&id)
}

/// Delete a single embedding.
#[tauri::command]
fn delete_embedding(app: tauri::AppHandle, id: String) -> Result<(), String> {
    db_ref(&app)?.delete_embedding(&id)
}

/// Ids of people with ≥1 approved embedding — the directory's "enrolled" badge.
#[tauri::command]
fn enrolled_person_ids(app: tauri::AppHandle) -> Vec<String> {
    match app.state::<DbState>().lock().unwrap().as_ref() {
        Some(db) => db.enrolled_person_ids(),
        None => Vec::new(),
    }
}

/// Result of a recognition probe: the **query** embedding (so the UI can draw the
/// target's point-cloud), plus the best `match` within `max_distance` (or none).
#[derive(Serialize)]
struct RecognizeDetail {
    query: Vec<f32>,
    #[serde(rename = "match")]
    matched: Option<RecognitionResult>,
}

/// Recognise a face: embed `image` (base64, optionally a data URL) and match it
/// against the approved embeddings. Returns the query embedding + the best match.
#[tauri::command]
fn recognize_face(
    app: tauri::AppHandle,
    image: String,
    max_distance: Option<f32>,
) -> Result<RecognizeDetail, String> {
    let b64 = image.rsplit(',').next().unwrap_or(&image);
    let bytes = B64.decode(b64.trim()).map_err(|e| format!("decode image: {e}"))?;
    let query = app_core::agent::face::embed(&bytes)?;
    let matches = db_ref(&app)?.recognize(&query, 5);
    let matched = persons::best_match(&matches, max_distance.unwrap_or(0.4));
    Ok(RecognizeDetail { query, matched })
}

/// Run an ONVIF WS-Discovery sweep on a blocking task (it's a sync UDP collect).
#[tauri::command]
async fn discover_devices(app: tauri::AppHandle) -> Vec<DiscoveredDevice> {
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
    use tauri::Emitter;
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
fn spawn_device_rescan(app: &tauri::AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(3600));
        let found = onvif::discovery::discover();
        if !found.is_empty() {
            reconcile_devices(&app, &found);
        }
    });
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
                let _ = persist_devices(&app, &devices); // best-effort self-heal
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
        persist_devices(&app, &devices)?;
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
        db_ref(&app).ok().as_ref(),
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
    let removed = alerts::delete(&state, db_ref(&app).ok().as_ref(), id);
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
    db_ref(&app)?.save_telegram(&cfg.bot_token, &cfg.pairing_code);
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
    let (label, _) = notification::set_enabled_by_id(&state, db_ref(&app).ok().as_ref(), chat_id, enabled)
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
    let new_label = notification::rename_target(&state, db_ref(&app).ok().as_ref(), chat_id, &label)?;
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
    let removed = notification::delete_target(&state, db_ref(&app).ok().as_ref(), chat_id)
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
    let db = db_ref(&app).ok();
    let result = tauri::async_runtime::spawn_blocking(move || {
        alerts::deliver(&alert_store, db.as_ref(), &notif_store, &cfg, id)
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
        out.push_str(&format!("  {}: {}\n", app_core::go2rtc::stream_name(&d.ip), url));
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
            let name = app_core::go2rtc::stream_name(&d.ip);
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

/// Start or stop the keep-warm poller to match `Settings.cameras_always_warm`.
/// When on, a background thread pings each camera's go2rtc frame every ~2s so the
/// shared feed stays connected and snapshots are instant even after idle. When off,
/// it signals the running poller to stop. Idempotent — safe to call repeatedly
/// (from `setup` and whenever settings change).
fn reconcile_keepwarm(app: &tauri::AppHandle) {
    use std::sync::atomic::{AtomicBool, Ordering};
    let want = load_settings(&settings_file(app)).cameras_always_warm;
    let handle = app.state::<WarmHandle>();
    let running = handle.lock().unwrap().is_some();

    if want && !running {
        let stop = Arc::new(AtomicBool::new(false));
        *handle.lock().unwrap() = Some(stop.clone());
        let app = app.clone();
        std::thread::spawn(move || {
            log::info!("keep-warm: keeping camera feeds always on");
            while !stop.load(Ordering::Relaxed) {
                let devices = app.state::<DeviceStore>().lock().unwrap().clone();
                for d in &devices {
                    let has_stream =
                        d.rtsp_url.as_deref().map(|u| !u.trim().is_empty()).unwrap_or(false);
                    if has_stream {
                        let _ = app_core::go2rtc::frame(&d.ip); // discard — just keep it warm
                    }
                }
                std::thread::sleep(Duration::from_secs(2));
            }
            log::info!("keep-warm: stopped");
        });
    } else if !want && running {
        if let Some(stop) = handle.lock().unwrap().take() {
            stop.store(true, Ordering::Relaxed);
        }
    }
}

// ---- Map (3D floor plan) ---------------------------------------------------

/// The saved floor plan (rooms + assigned cameras). Stateless: the frontend owns
/// the working copy and saves the whole array back.
#[tauri::command]
fn load_map(app: tauri::AppHandle) -> Vec<MapRoom> {
    db_ref(&app).map(|db| db.load_map()).unwrap_or_default()
}

/// Persist the whole floor-plan array to Postgres (replace-all).
#[tauri::command]
fn save_map(app: tauri::AppHandle, rooms: Vec<MapRoom>) -> Result<(), String> {
    db_ref(&app)?.save_map(&rooms);
    log::info!("save_map: persisted {} room(s)", rooms.len());
    Ok(())
}

// ---- Tool runtime (shared by chat + workflows) -----------------------------

/// Build a web `ToolContext` whose UI-effect channel is drained to Tauri events
/// on a background thread (the thread ends when the returned context is dropped,
/// closing the Sender). Shared by the chat agent and the workflows engine so tool
/// side-effects — snapshots, store changes, open-live — surface identically.
///
/// Returns `None` if Postgres isn't connected yet (the device tools need the DB
/// handle). In practice the app is past the splash before any tool runs; this
/// guards the brief startup window where the workflow poller could fire first.
pub(crate) fn build_run_context(app: &tauri::AppHandle) -> Option<app_core::agent::ToolContext> {
    use app_core::agent::ToolUiEvent;
    use tauri::Emitter;

    let db = app.state::<DbState>().lock().unwrap().clone()?;
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
                ToolUiEvent::Print { message, level, on_screen } => {
                    let _ = emit_app.emit(
                        "workflow-print",
                        serde_json::json!({ "message": message, "level": level, "on_screen": on_screen }),
                    );
                }
                ToolUiEvent::ShowImage { graph, node, label, jpeg } => {
                    let data = B64.encode(&jpeg);
                    let _ = emit_app.emit(
                        "workflow-image",
                        serde_json::json!({ "graph": graph, "node": node, "label": label, "data": data }),
                    );
                }
            }
        }
    });

    Some(app_core::agent::ToolContext {
        devices,
        db,
        alerts,
        notif,
        notif_cfg: resolve_telegram_config(app),
        ui_tx: tx,
    })
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
    let ctx = build_run_context(&app).ok_or("Sentinel's database isn't ready yet.")?;

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
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            // Face models (YOLOv8-face + ArcFace) load at runtime from here. Drop the
            // ONNX files (`yolov8_face.onnx`, `arcface.onnx`) into this dir and
            // recognition switches from the placeholder to real — no rebuild. Absent →
            // placeholder fallback (see `app_core::agent::face`).
            let models_dir = app
                .path()
                .app_data_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .join("models");
            let _ = std::fs::create_dir_all(&models_dir);
            std::env::set_var("SENTINEL_MODELS_DIR", &models_dir);
            log::info!("Face models dir: {}", models_dir.display());

            // Devices live in Postgres. The store starts empty and is hydrated by
            // `try_connect_db` (here, best-effort) or by the splash's `boot_status`
            // retry loop once the database is reachable — there is no JSON fallback.
            let db_state: DbState = Arc::new(Mutex::new(None));
            app.manage(db_state);
            let store: DeviceStore = Arc::new(Mutex::new(Vec::new()));
            app.manage(store);
            // People: hydrated from the DB alongside devices by `try_connect_db`.
            let person_store: PersonStore = Arc::new(Mutex::new(Vec::new()));
            app.manage(person_store);

            // Alerts live in Postgres. The store is managed empty BEFORE the DB connect so
            // `try_connect_db` can hydrate it (with a one-time import from any legacy
            // alerts.json) — same pattern as devices/persons.
            let alert_store: AlertStore = Arc::new(Mutex::new(Vec::new()));
            app.manage(alert_store);

            // Beacons (Telegram targets) live in Postgres; managed empty BEFORE the DB
            // connect so try_connect_db can hydrate them (with a one-time import from any
            // legacy notifications.json) — same pattern as devices/persons/alerts.
            let notif_store: NotificationStore = Arc::new(Mutex::new(Vec::new()));
            app.manage(notif_store);

            // Workflow graph store, managed empty BEFORE the DB connect so
            // try_connect_db can hydrate it (the workflows slice owns the details).
            features::workflows::manage_empty_store(&app.handle());

            if let Err(e) = try_connect_db(&app.handle()) {
                log::warn!("Postgres not ready at startup ({e}); splash will retry");
            }

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

            // Keep-warm: managed stop-flag for the optional "always warm" poller that
            // keeps each camera's shared go2rtc feed connected (started below if the
            // setting is on, and toggled live from `set_settings`).
            let warm: WarmHandle = Arc::new(Mutex::new(None));
            app.manage(warm);

            // Chat: a single managed conversation history for Bishop's agent.
            let history: ChatHistory = Arc::new(Mutex::new(Vec::new()));
            app.manage(history);

            // Workflows engine + activity map + trigger poller (the slice seeds the
            // engine from the current alert max so the backlog doesn't fire OnNewAlert).
            features::workflows::start(&app.handle());

            // Hourly ONVIF rescan → reconcile IP changes (DHCP / power-cycle).
            spawn_device_rescan(&app.handle());
            let handle = app.handle().clone();
            let devices = handle.state::<DeviceStore>().lock().unwrap().clone();
            let config = go2rtc_config_file(&handle);
            spawn_go2rtc(&handle, &config, &build_go2rtc_yaml(&devices));

            // Start the keep-warm poller if the user has "always warm" enabled.
            reconcile_keepwarm(&app.handle());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            boot_status,
            get_settings,
            set_settings,
            list_devices,
            save_device,
            delete_device,
            rename_device,
            list_people,
            person_permission_kinds,
            save_person,
            delete_person,
            enroll_face,
            list_embeddings,
            approve_embedding,
            delete_embedding,
            enrolled_person_ids,
            recognize_face,
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
            features::workflows::load_graphs,
            features::workflows::run_states,
            features::workflows::node_catalog,
            features::workflows::create_graph,
            features::workflows::rename_graph,
            features::workflows::delete_graph,
            features::workflows::set_graph_trigger,
            features::workflows::set_graph_concurrency,
            features::workflows::set_graph_enabled,
            features::workflows::set_graph_run_orphans,
            features::workflows::set_graph_debug,
            features::workflows::set_graph_viewport,
            features::workflows::add_node,
            features::workflows::remove_node,
            features::workflows::move_node,
            features::workflows::set_node_config,
            features::workflows::add_edge,
            features::workflows::remove_edge,
            features::workflows::run_graph,
            voice_ready,
            speak,
            transcribe,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
