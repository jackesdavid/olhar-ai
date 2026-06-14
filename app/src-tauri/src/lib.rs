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

use app_core::alerts::AlertStore;
use app_core::db::Db;
use app_core::devices::DeviceStore;
use app_core::notification::NotificationStore;
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use tauri::Manager;

/// Per-feature Tauri adapters (commands + pollers + state lifecycle).
mod features;

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
    log::info!("Postgres connected; migrations applied");
    // Devices (camera registry) — the devices slice owns its hydration.
    features::devices::hydrate(app, &db);
    // People (hierarchy + face embeddings) — the persons slice owns its hydration.
    features::people::hydrate(app, &db);
    // The alert log lives in Postgres — the alerts slice owns its hydration + import.
    features::alerts::hydrate(app, &db);
    // Beacons + Telegram config — the notification slice owns its hydration + import.
    features::notifications::hydrate(app, &db);
    // Workflow graphs live in Postgres too — the workflows slice owns its hydration.
    features::workflows::hydrate(app, &db);
    // Map rooms live in Postgres (read on demand, no store) — the map slice owns
    // its one-time legacy-JSON import.
    features::map::hydrate(app, &db);

    *state.lock().unwrap() = Some(db);
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

/// Splash boot probe: ensure Postgres is connected + migrated. The splash calls
/// this on mount and retries until it returns `Ok`, then reveals the app.
#[tauri::command]
fn boot_status(app: tauri::AppHandle) -> Result<(), String> {
    try_connect_db(&app)
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

    Some(app_core::agent::ToolContext::new_local(
        devices,
        db,
        app_core::tenancy::DEFAULT_TENANT_ID,
        alerts,
        notif,
        features::notifications::resolve_telegram_config(app),
        tx,
    ))
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
            // Devices (camera registry): managed empty BEFORE the DB connect so
            // try_connect_db can hydrate it (the devices slice owns the details).
            features::devices::manage_empty_store(&app.handle());
            // People: managed empty BEFORE the DB connect so try_connect_db can
            // hydrate it (the persons slice owns the details).
            features::people::manage_empty_store(&app.handle());

            // Alert store, managed empty BEFORE the DB connect so try_connect_db can
            // hydrate it (the alerts slice owns the details).
            features::alerts::manage_empty_store(&app.handle());

            // Beacons (Telegram targets) live in Postgres; managed empty BEFORE the DB
            // connect so try_connect_db can hydrate them (with a one-time import from any
            // legacy notifications.json) — same pattern as devices/persons/alerts.
            features::notifications::manage(&app.handle());

            // Workflow graph store, managed empty BEFORE the DB connect so
            // try_connect_db can hydrate it (the workflows slice owns the details).
            features::workflows::manage_empty_store(&app.handle());

            if let Err(e) = try_connect_db(&app.handle()) {
                log::warn!("Postgres not ready at startup ({e}); splash will retry");
            }

            // Pairing poller: managed stop-handle so `set_telegram_config` can
            // restart the inbound `/pair <code>` listener without an app restart.
            features::notifications::start_pairing(&app.handle());

            // Live feed + snapshots: the on-prem sentinel-edge service owns go2rtc and
            // is the only thing that touches cameras — the stream slice is a thin
            // client of it (EDGE_URL), so there's nothing to start here.

            // Chat: a single managed conversation history for Bishop's agent.
            features::chat::manage(&app.handle());

            // Workflows: client of sentinel-flow — bridges its SSE to Tauri events.
            features::workflows::start(&app.handle());

            // Hourly ONVIF rescan → reconcile IP changes (DHCP / power-cycle).
            features::devices::spawn_device_rescan(&app.handle());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            boot_status,
            features::settings::get_settings,
            features::settings::set_settings,
            features::devices::list_devices,
            features::devices::save_device,
            features::devices::delete_device,
            features::devices::rename_device,
            features::people::list_people,
            features::people::person_permission_kinds,
            features::people::save_person,
            features::people::delete_person,
            features::people::enroll_face,
            features::people::list_embeddings,
            features::people::approve_embedding,
            features::people::delete_embedding,
            features::people::enrolled_person_ids,
            features::people::recognize_face,
            features::devices::discover_devices,
            features::devices::test_credentials,
            features::devices::validate_device,
            features::stream::capture_snapshot,
            features::stream::ensure_streams,
            features::alerts::list_alerts,
            features::alerts::create_alert,
            features::alerts::delete_alert,
            features::alerts::alert_stats,
            features::notifications::get_telegram_config,
            features::notifications::set_telegram_config,
            features::notifications::list_beacons,
            features::notifications::toggle_beacon,
            features::notifications::rename_beacon,
            features::notifications::delete_beacon,
            features::notifications::test_beacon,
            features::alerts::deliver_alert,
            features::map::load_map,
            features::map::save_map,
            features::chat::chat_ready,
            features::chat::reset_chat,
            features::chat::send_chat,
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
            features::voice::voice_ready,
            features::voice::speak,
            features::voice::transcribe,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
