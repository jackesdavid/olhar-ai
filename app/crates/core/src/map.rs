//! Map data: the persisted floor plan (`map.json`). Copied from the ratatui
//! `src/map/domain.rs` — the `MapRoom`/`RoomCamera` structs (and their serde
//! camera-compat shim). Rooms are axis-aligned rectangles on an integer grid
//! with a **y-down, top-left origin** (x grows right, y grows down). Each room
//! carries real geometry so contacts can be placed at positions inside it.
//!
//! The ratatui `MapState` (the canvas/viewport/zoom editor) and all ratatui bits
//! are DROPPED — the web app owns its own (Threlte) editor and working copy.
//! Persistence is path-parameterized (no hardcoded `map.json` in the cwd) so the
//! Tauri shell writes under the OS app-data dir.
use std::path::Path;

use serde::{Deserialize, Serialize};

/// Size of a freshly added room (used by the frontend "Add room" default).
pub const DEFAULT_ROOM_W: i32 = 6;
pub const DEFAULT_ROOM_H: i32 = 4;

/// A camera assigned to a room, positioned by an offset within the room
/// (`dx, dy` in cells from the room's top-left). Absolute grid position is
/// `(room.x + dx, room.y + dy)`. References the device by its stable `ip`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoomCamera {
    pub ip: String,
    #[serde(default)]
    pub dx: i32,
    #[serde(default)]
    pub dy: i32,
}

/// One room on the floor plan: an axis-aligned rectangle (grid units, y-down,
/// top-left origin) with a free-text name and the cameras assigned to cover it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MapRoom {
    pub id: u32,
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    /// Accepts both the new `[{ip,dx,dy}]` form and the legacy `["ip"]` form;
    /// legacy entries get centered in the room on load.
    #[serde(default, deserialize_with = "de_cameras")]
    pub cameras: Vec<RoomCamera>,
}

/// Deserialize the `cameras` field, accepting either the new object form or the
/// old bare-ip-string form (legacy strings become sentinel-positioned cameras,
/// re-centered by [`clamp_cam`] on load).
fn de_cameras<'de, D>(d: D) -> Result<Vec<RoomCamera>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Entry {
        Ip(String),
        Full(RoomCamera),
    }
    let entries = Vec::<Entry>::deserialize(d)?;
    Ok(entries
        .into_iter()
        .map(|e| match e {
            Entry::Ip(ip) => RoomCamera { ip, dx: -1, dy: -1 }, // sentinel → centered on load
            Entry::Full(c) => c,
        })
        .collect())
}

/// Snap a camera's offset inside its `w×h` room; a negative (sentinel) offset
/// centers it.
fn clamp_cam(cam: &mut RoomCamera, w: i32, h: i32) {
    if cam.dx < 0 || cam.dy < 0 {
        cam.dx = w / 2;
        cam.dy = h / 2;
    }
    cam.dx = cam.dx.clamp(0, (w - 1).max(0));
    cam.dy = cam.dy.clamp(0, (h - 1).max(0));
}

impl MapRoom {
    /// Centre of the room in grid coords — for label / marker placement.
    pub fn center(&self) -> (f64, f64) {
        (self.x as f64 + self.w as f64 / 2.0, self.y as f64 + self.h as f64 / 2.0)
    }
}

/// The next free room id (max existing + 1), for the frontend's "Add room".
pub fn next_id(rooms: &[MapRoom]) -> u32 {
    rooms.iter().map(|r| r.id).max().map_or(0, |m| m + 1)
}

/// Snap every room's camera offsets into its rectangle (centers legacy/sentinel
/// entries). Applied after loading from any source (JSON file or Postgres).
pub fn normalize_rooms(rooms: &mut [MapRoom]) {
    for r in rooms {
        let (w, h) = (r.w, r.h);
        for cam in &mut r.cameras {
            clamp_cam(cam, w, h);
        }
    }
}

/// Read a legacy `map.json` from `path` (empty if missing/unreadable), normalized.
/// Kept only for the one-time JSON→Postgres import on first DB boot — the floor plan
/// now lives in the shared `map_rooms` table (see [`crate::db::Db::load_map`]).
pub fn load_map(path: &Path) -> Vec<MapRoom> {
    let mut rooms: Vec<MapRoom> = std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    normalize_rooms(&mut rooms);
    rooms
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk(id: u32) -> MapRoom {
        MapRoom {
            id,
            name: format!("Room {id}"),
            x: 0,
            y: 0,
            w: DEFAULT_ROOM_W,
            h: DEFAULT_ROOM_H,
            cameras: vec![],
        }
    }

    #[test]
    fn missing_file_is_empty() {
        let path = std::env::temp_dir().join("app_core_map_nonexistent_xyz.json");
        let _ = std::fs::remove_file(&path);
        assert!(load_map(&path).is_empty());
    }

    #[test]
    fn next_id_is_max_plus_one() {
        assert_eq!(next_id(&[]), 0);
        assert_eq!(next_id(&[mk(0), mk(5), mk(2)]), 6);
    }

    #[test]
    fn legacy_cameras_get_centered() {
        let json = r#"[{"id":0,"name":"R","x":0,"y":0,"w":6,"h":4,"cameras":["10.0.0.1"]}]"#;
        let dir = std::env::temp_dir().join("app_core_map_legacy_test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("map.json");
        std::fs::write(&path, json).unwrap();
        let rooms = load_map(&path);
        assert_eq!(rooms[0].cameras[0].dx, 3);
        assert_eq!(rooms[0].cameras[0].dy, 2);
        let _ = std::fs::remove_file(&path);
    }
}
