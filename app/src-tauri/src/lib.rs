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
use app_core::config::{load_settings, save_settings, Settings};
use app_core::db::Db;
use app_core::devices::DeviceStore;
use app_core::map::{self, load_map as core_load_map, MapRoom};
use app_core::notification::NotificationStore;
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use tauri::Manager;

/// Per-feature Tauri adapters (commands + pollers + state lifecycle).
mod features;

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

    // Map rooms + Telegram config also live in Postgres (read on demand, no store).
    // One-time non-destructive import from legacy JSON files when the DB is empty.
    if map::repo::load(&db).is_empty() {
        let from_json = core_load_map(&map_file(app));
        if !from_json.is_empty() {
            map::repo::save(&db, &from_json);
            log::info!("Imported {} map room(s) from map.json into Postgres", from_json.len());
        }
    }
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

/// The web app's floor-plan file, under the platform app-data dir (created if missing).
fn map_file(app: &tauri::AppHandle) -> PathBuf {
    let dir = app
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| PathBuf::from("."));
    let _ = std::fs::create_dir_all(&dir);
    dir.join("map.json")
}

#[tauri::command]
fn get_settings(app: tauri::AppHandle) -> Settings {
    load_settings(&settings_file(&app))
}

#[tauri::command]
fn set_settings(app: tauri::AppHandle, settings: Settings) {
    save_settings(&settings_file(&app), &settings);
    // React to the "keep camera feeds always on" toggle without an app restart.
    features::stream::reconcile_keepwarm(&app);
}

// ---- Map (3D floor plan) ---------------------------------------------------

/// The saved floor plan (rooms + assigned cameras). Stateless: the frontend owns
/// the working copy and saves the whole array back.
#[tauri::command]
fn load_map(app: tauri::AppHandle) -> Vec<MapRoom> {
    db_ref(&app).map(|db| map::repo::load(&db)).unwrap_or_default()
}

/// Persist the whole floor-plan array to Postgres (replace-all).
#[tauri::command]
fn save_map(app: tauri::AppHandle, rooms: Vec<MapRoom>) -> Result<(), String> {
    map::repo::save(&db_ref(&app)?, &rooms);
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
        notif_cfg: features::notifications::resolve_telegram_config(app),
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

            // Live feed + keep-warm: the stream slice manages the go2rtc sidecar
            // handle and the keep-warm stop-flag (started below via `stream::start`).
            features::stream::manage(&app.handle());

            // Chat: a single managed conversation history for Bishop's agent.
            let history: ChatHistory = Arc::new(Mutex::new(Vec::new()));
            app.manage(history);

            // Workflows engine + activity map + trigger poller (the slice seeds the
            // engine from the current alert max so the backlog doesn't fire OnNewAlert).
            features::workflows::start(&app.handle());

            // Hourly ONVIF rescan → reconcile IP changes (DHCP / power-cycle).
            features::devices::spawn_device_rescan(&app.handle());

            // Best-effort startup: (re)start go2rtc for the current devices so
            // streams are ready on launch, then start keep-warm if enabled.
            features::stream::start(&app.handle());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            boot_status,
            get_settings,
            set_settings,
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
