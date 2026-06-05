//! Phase-0a spike — prove the Tauri 2 (Rust backend) ↔ SvelteKit (webview) stack by
//! reusing Bishop's alerts model end-to-end: real Rust logic behind a
//! `#[tauri::command]`, rendered by the Svelte UI.
//!
//! NOTE: this is a SPIKE COPY of the alert types from `bishop::alerts::domain`. In the
//! real migration (Phase 1) these move into a shared `crates/core` that BOTH the existing
//! ratatui app and this `src-tauri` depend on — proving exactly that the logic is reused
//! verbatim, not rewritten.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AlertLevel {
    Debug,
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Delivery {
    pub target: String,
    pub chat_id: i64,
    pub at: String,
    pub ok: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Alert {
    pub id: u64,
    pub level: AlertLevel,
    pub message: String,
    #[serde(default)]
    pub room_id: Option<u32>,
    #[serde(default)]
    pub room_name: Option<String>,
    pub source: String,
    pub created_at: String,
    #[serde(default)]
    pub deliveries: Vec<Delivery>,
}

/// Mirrors `bishop::alerts::domain::load_alerts` — deserialize `alerts.json`. Tries a few
/// candidate paths (the spike runs with cwd = `src-tauri` under `tauri dev`, so the repo
/// root is two levels up) so the demo reads Bishop's REAL alert data.
fn load_alerts() -> Vec<Alert> {
    let candidates = [
        PathBuf::from("alerts.json"),
        PathBuf::from("../alerts.json"),
        PathBuf::from("../../alerts.json"), // repo root, from web-spike-0a/src-tauri
    ];
    for p in candidates {
        if let Ok(s) = std::fs::read_to_string(&p) {
            if let Ok(v) = serde_json::from_str::<Vec<Alert>>(&s) {
                return v;
            }
        }
    }
    Vec::new()
}

/// The command the Svelte UI invokes: returns Bishop's alerts (newest first) as typed JSON.
#[tauri::command]
fn list_alerts() -> Vec<Alert> {
    let mut alerts = load_alerts();
    alerts.sort_by(|a, b| b.id.cmp(&a.id));
    alerts
}

// ---- Map rooms (Phase-0c: Bishop's floor-plan data → Threlte 3D scene) ------
// Spike copy of `bishop::map::domain::{MapRoom, RoomCamera}`.

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoomCamera {
    pub ip: String,
    #[serde(default)]
    pub dx: i32,
    #[serde(default)]
    pub dy: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MapRoom {
    pub id: u32,
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    #[serde(default)]
    pub cameras: Vec<RoomCamera>,
}

fn load_rooms() -> Vec<MapRoom> {
    let candidates = [
        PathBuf::from("map.json"),
        PathBuf::from("../map.json"),
        PathBuf::from("../../map.json"), // repo root, from web-spike-0a/src-tauri
    ];
    for p in candidates {
        if let Ok(s) = std::fs::read_to_string(&p) {
            if let Ok(v) = serde_json::from_str::<Vec<MapRoom>>(&s) {
                return v;
            }
        }
    }
    Vec::new()
}

/// Returns Bishop's floor-plan rooms (from `map.json`) for the 3D scene.
#[tauri::command]
fn list_rooms() -> Vec<MapRoom> {
    load_rooms()
}

/// Phase-0b: best-effort start of the **go2rtc** sidecar (an RTSP→WebRTC gateway) pointing
/// at `go2rtc.yaml`. The webview then plays the camera via go2rtc's WebRTC player — the
/// least-broken cross-platform path for live video in a Tauri webview.
///
/// SPIKE shortcut: spawned via `std::process` (cwd = `src-tauri` under `tauri dev`).
/// Production uses a Tauri **externalBin sidecar** so it's bundled + lifecycle-managed.
/// Drop the binary at `binaries/go2rtc` (see `binaries/DOWNLOAD.md`) and set your camera's
/// RTSP URL in `go2rtc.yaml`.
fn spawn_go2rtc() {
    let bin = std::path::Path::new("binaries/go2rtc");
    if !bin.exists() {
        eprintln!(
            "[spike] go2rtc not found at {bin:?} — live video disabled. See binaries/DOWNLOAD.md"
        );
        return;
    }
    match std::process::Command::new(bin)
        .args(["-config", "go2rtc.yaml"])
        .spawn()
    {
        Ok(_) => eprintln!("[spike] go2rtc started → http://localhost:1984"),
        Err(e) => eprintln!("[spike] failed to start go2rtc: {e}"),
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|_app| {
            spawn_go2rtc();
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![list_alerts, list_rooms])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
