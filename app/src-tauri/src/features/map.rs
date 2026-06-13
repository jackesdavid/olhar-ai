//! Map — Tauri adapter for the floor-plan slice: the 3D room layout with assigned
//! cameras. Stateless (no managed store — the frontend owns the working copy and
//! saves the whole array back). The domain lives in `app_core::map`; persistence in
//! `app_core::map::repo`.
use std::path::PathBuf;

use tauri::Manager;

use app_core::db::Db;
use app_core::map::{self, load_map as core_load_map, MapRoom};

use crate::db_ref;

/// The web app's legacy floor-plan file, under the platform app-data dir (the
/// one-time import source on first DB boot only).
fn map_file(app: &tauri::AppHandle) -> PathBuf {
    let dir = app
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| PathBuf::from("."));
    let _ = std::fs::create_dir_all(&dir);
    dir.join("map.json")
}

// ---- lifecycle (called by lib.rs try_connect_db) ----------------------------

/// One-time non-destructive import of any legacy `map.json` into Postgres when the
/// `map_rooms` table is empty. Read on demand thereafter — no managed store.
pub fn hydrate(app: &tauri::AppHandle, db: &Db) {
    if map::repo::load(db).is_empty() {
        let from_json = core_load_map(&map_file(app));
        if !from_json.is_empty() {
            map::repo::save(db, &from_json);
            log::info!("Imported {} map room(s) from map.json into Postgres", from_json.len());
        }
    }
}

// ---- commands ---------------------------------------------------------------

/// The saved floor plan (rooms + assigned cameras). Stateless: the frontend owns
/// the working copy and saves the whole array back.
#[tauri::command]
pub fn load_map(app: tauri::AppHandle) -> Vec<MapRoom> {
    db_ref(&app).map(|db| map::repo::load(&db)).unwrap_or_default()
}

/// Persist the whole floor-plan array to Postgres (replace-all).
#[tauri::command]
pub fn save_map(app: tauri::AppHandle, rooms: Vec<MapRoom>) -> Result<(), String> {
    map::repo::save(&db_ref(&app)?, &rooms);
    log::info!("save_map: persisted {} room(s)", rooms.len());
    Ok(())
}
