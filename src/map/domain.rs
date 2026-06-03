//! Map data: the persisted floor plan (`map.json`) and the transient editor
//! state. Rooms are axis-aligned rectangles on an integer grid with a **y-down,
//! top-left origin** (x grows right, y grows down). Each room carries real
//! geometry so future work can place detected contacts at positions inside it.
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Floor-plan grid size in cells. The canvas maps one grid cell to one unit.
pub const GRID_W: i32 = 32;
pub const GRID_H: i32 = 18;

/// Size of a freshly added room.
pub const DEFAULT_ROOM_W: i32 = 6;
pub const DEFAULT_ROOM_H: i32 = 4;

/// One room on the floor plan: an axis-aligned rectangle (grid units, y-down,
/// top-left origin) with a free-text name and the cameras assigned to cover it.
/// `cameras` references devices by their stable `ip` (see [`crate::scan::Device`]).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MapRoom {
    pub id: u32,
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    #[serde(default)]
    pub cameras: Vec<String>,
}

impl MapRoom {
    /// Centre of the room in grid coords — for label / marker placement.
    pub fn center(&self) -> (f64, f64) {
        (self.x as f64 + self.w as f64 / 2.0, self.y as f64 + self.h as f64 / 2.0)
    }
}

/// Which sub-mode the editor is in. Arrow keys are reused per-mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MapMode {
    Browse,
    Move,
    Resize,
    Rename,
    Cameras,
}

/// Transient editor state. The room data is owned here (single-owner — unlike
/// the scan device list there's no background thread mutating it).
pub struct MapState {
    pub rooms: Vec<MapRoom>,
    pub selected: usize,
    pub mode: MapMode,
    /// Highlighted row in the Cameras device checklist.
    pub cam_cursor: usize,
    /// One-line feedback shown in amber, like `AlertsScreen.status`.
    pub status: String,
    /// Monotonic id source for new rooms.
    pub next_id: u32,
    /// Name snapshot so Esc can cancel a rename.
    pub(super) rename_backup: String,
}

impl MapState {
    pub fn new() -> Self {
        let rooms = load_map();
        let next_id = rooms.iter().map(|r| r.id).max().map_or(0, |m| m + 1);
        MapState {
            rooms,
            selected: 0,
            mode: MapMode::Browse,
            cam_cursor: 0,
            status: String::new(),
            next_id,
            rename_backup: String::new(),
        }
    }
}

impl Default for MapState {
    fn default() -> Self {
        Self::new()
    }
}

fn map_path() -> PathBuf {
    PathBuf::from("map.json")
}

pub fn load_map() -> Vec<MapRoom> {
    std::fs::read_to_string(map_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save_map(rooms: &[MapRoom]) {
    if let Ok(json) = serde_json::to_string_pretty(rooms) {
        let _ = std::fs::write(map_path(), json);
    }
}
