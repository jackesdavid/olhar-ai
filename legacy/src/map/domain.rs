//! Map data: the persisted floor plan (`map.json`) and the transient editor
//! state. Rooms are axis-aligned rectangles on an integer grid with a **y-down,
//! top-left origin** (x grows right, y grows down). Each room carries real
//! geometry so future work can place detected contacts at positions inside it.
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Floor-plan **world** size in cells — the full coordinate space rooms can
/// occupy. The viewport (see [`MapState::viewport`]) shows a window into it.
pub const GRID_W: i32 = 64;
pub const GRID_H: i32 = 36;

/// The default viewport span (cells shown) and the aspect it holds while zooming.
pub const DEFAULT_VIEW_W: i32 = 32;
pub const DEFAULT_VIEW_H: i32 = 18;

/// Discrete zoom levels, as visible widths in cells (height derived from the
/// aspect). Index `DEFAULT_ZOOM` (→ 32 wide) reproduces the pre-viewport view.
pub const ZOOM_SPANS: [i32; 5] = [16, 24, 32, 48, 64];
pub const DEFAULT_ZOOM: usize = 2;

/// Size of a freshly added room.
pub const DEFAULT_ROOM_W: i32 = 6;
pub const DEFAULT_ROOM_H: i32 = 4;

/// A camera assigned to a room, positioned by an offset within the room
/// (`dx, dy` in cells from the room's top-left). Absolute grid position is
/// `(room.x + dx, room.y + dy)`; the offset is always clamped to the room, so a
/// camera can never leave it and rides along when the room moves/resizes.
/// References the device by its stable `ip` (see [`crate::scan::Device`]).
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

/// Which sub-mode the editor is in. Arrow keys are reused per-mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MapMode {
    Browse,
    Move,
    Resize,
    Rename,
    Cameras,
    /// Positioning a single assigned camera within the selected room.
    CamPlace,
    Pan,
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
    /// Top-left of the visible window in world coords (y-down, like rooms).
    pub view_x: i32,
    pub view_y: i32,
    /// Index into [`ZOOM_SPANS`] — the current zoom level.
    pub zoom: usize,
}

impl MapState {
    pub fn new() -> Self {
        let rooms = load_map();
        let next_id = rooms.iter().map(|r| r.id).max().map_or(0, |m| m + 1);
        let (view_x, view_y, zoom) = load_view();
        MapState {
            rooms,
            selected: 0,
            mode: MapMode::Browse,
            cam_cursor: 0,
            status: String::new(),
            next_id,
            rename_backup: String::new(),
            view_x,
            view_y,
            zoom,
        }
    }

    /// Persist the current viewport to `mapview.json`.
    fn persist_view(&self) {
        save_view(self.view_x, self.view_y, self.zoom);
    }

    /// The visible-window span `(width, height)` in cells for a zoom index,
    /// holding the default-view aspect and capped to the world.
    fn span(zoom: usize) -> (i32, i32) {
        let vw = ZOOM_SPANS[zoom.min(ZOOM_SPANS.len() - 1)].min(GRID_W);
        let vh = (vw * DEFAULT_VIEW_H / DEFAULT_VIEW_W).clamp(1, GRID_H);
        (vw, vh)
    }

    /// The current viewport `(x, y, w, h)` in world cells, clamped into the world.
    pub fn viewport(&self) -> (i32, i32, i32, i32) {
        let (vw, vh) = Self::span(self.zoom);
        let vx = self.view_x.clamp(0, (GRID_W - vw).max(0));
        let vy = self.view_y.clamp(0, (GRID_H - vh).max(0));
        (vx, vy, vw, vh)
    }

    /// Snap the stored offset back inside the world for the current zoom.
    fn clamp_view(&mut self) {
        let (vx, vy, _, _) = self.viewport();
        self.view_x = vx;
        self.view_y = vy;
    }

    /// Set the zoom level, keeping the viewport centre fixed, then re-clamp.
    fn set_zoom(&mut self, zoom: usize) {
        let (vx, vy, vw, vh) = self.viewport();
        let (cx, cy) = (vx + vw / 2, vy + vh / 2);
        self.zoom = zoom.min(ZOOM_SPANS.len() - 1);
        let (nw, nh) = Self::span(self.zoom);
        self.view_x = cx - nw / 2;
        self.view_y = cy - nh / 2;
        self.clamp_view();
        self.persist_view();
    }

    pub fn zoom_in(&mut self) {
        self.set_zoom(self.zoom.saturating_sub(1));
    }

    pub fn zoom_out(&mut self) {
        self.set_zoom(self.zoom + 1); // set_zoom caps at the last level
    }

    /// Pan the viewport by `(dx, dy)` steps (a couple of cells each), clamped.
    pub fn pan(&mut self, dx: i32, dy: i32) {
        const STEP: i32 = 2;
        self.view_x += dx * STEP;
        self.view_y += dy * STEP;
        self.clamp_view();
        self.persist_view();
    }

    /// Frame all rooms: smallest zoom that contains their bounding box, centred
    /// on it. Resets to the default view when there are no rooms.
    pub fn fit(&mut self) {
        if self.rooms.is_empty() {
            self.zoom = DEFAULT_ZOOM;
            self.view_x = 0;
            self.view_y = 0;
            self.persist_view();
            return;
        }
        let minx = self.rooms.iter().map(|r| r.x).min().unwrap_or(0);
        let miny = self.rooms.iter().map(|r| r.y).min().unwrap_or(0);
        let maxx = self.rooms.iter().map(|r| r.x + r.w).max().unwrap_or(GRID_W);
        let maxy = self.rooms.iter().map(|r| r.y + r.h).max().unwrap_or(GRID_H);
        let (bw, bh) = (maxx - minx, maxy - miny);
        let zoom = (0..ZOOM_SPANS.len())
            .find(|&z| {
                let (w, h) = Self::span(z);
                w >= bw && h >= bh
            })
            .unwrap_or(ZOOM_SPANS.len() - 1);
        self.zoom = zoom;
        let (w, h) = Self::span(zoom);
        self.view_x = (minx + bw / 2) - w / 2;
        self.view_y = (miny + bh / 2) - h / 2;
        self.clamp_view();
        self.persist_view();
    }

    /// Auto-follow: if the selected room isn't fully inside the viewport, recentre
    /// the window on it. Called after selection and Move edits.
    pub fn ensure_visible(&mut self) {
        let Some(r) = self.rooms.get(self.selected) else { return };
        let (rx, ry, rw, rh) = (r.x, r.y, r.w, r.h);
        let (vx, vy, vw, vh) = self.viewport();
        let outside = rx < vx || ry < vy || rx + rw > vx + vw || ry + rh > vy + vh;
        if outside {
            self.view_x = (rx + rw / 2) - vw / 2;
            self.view_y = (ry + rh / 2) - vh / 2;
            self.clamp_view();
            self.persist_view();
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
    let mut rooms: Vec<MapRoom> = std::fs::read_to_string(map_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    // Normalize camera offsets into their rooms (centers legacy/sentinel entries).
    for r in &mut rooms {
        let (w, h) = (r.w, r.h);
        for cam in &mut r.cameras {
            clamp_cam(cam, w, h);
        }
    }
    rooms
}

pub fn save_map(rooms: &[MapRoom]) {
    if cfg!(test) {
        return; // never clobber the real map.json from unit tests
    }
    if let Ok(json) = serde_json::to_string_pretty(rooms) {
        let _ = std::fs::write(map_path(), json);
    }
}

/// The persisted viewport (pan offset + zoom), stored separately from the room
/// list so `map.json` stays a plain array of rooms.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ViewState {
    view_x: i32,
    view_y: i32,
    zoom: usize,
}

fn view_path() -> PathBuf {
    PathBuf::from("mapview.json")
}

/// Load the saved viewport, falling back to the origin at the default zoom when
/// there's no saved file (clamping is applied on read by [`MapState::viewport`]).
pub fn load_view() -> (i32, i32, usize) {
    match std::fs::read_to_string(view_path()).ok().and_then(|s| serde_json::from_str::<ViewState>(&s).ok()) {
        Some(v) => (v.view_x, v.view_y, v.zoom.min(ZOOM_SPANS.len() - 1)),
        None => (0, 0, DEFAULT_ZOOM),
    }
}

pub fn save_view(view_x: i32, view_y: i32, zoom: usize) {
    if cfg!(test) {
        return; // don't write mapview.json from unit tests
    }
    let v = ViewState { view_x, view_y, zoom };
    if let Ok(json) = serde_json::to_string_pretty(&v) {
        let _ = std::fs::write(view_path(), json);
    }
}
