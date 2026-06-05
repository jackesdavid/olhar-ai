//! Floor-plan editing logic: room selection, geometry edits (move/resize),
//! rename, and camera assignment. Pure state mutation — the device list is
//! passed in by `App` where needed, so this module never touches the scan store.
use super::domain::{
    save_map, MapMode, MapRoom, MapState, RoomCamera, DEFAULT_ROOM_H, DEFAULT_ROOM_W, GRID_H,
    GRID_W,
};

impl MapState {
    // ---- selection (mirrors BeaconsScreen::{next,prev,clamp}) --------------
    pub fn next(&mut self) {
        let n = self.rooms.len();
        if n > 0 {
            self.selected = (self.selected + 1) % n;
            self.ensure_visible();
        }
    }

    pub fn prev(&mut self) {
        let n = self.rooms.len();
        if n > 0 {
            self.selected = (self.selected + n - 1) % n;
            self.ensure_visible();
        }
    }

    fn clamp(&mut self) {
        let n = self.rooms.len();
        self.selected = if n == 0 { 0 } else { self.selected.min(n - 1) };
    }

    pub fn selected_room(&self) -> Option<&MapRoom> {
        self.rooms.get(self.selected)
    }

    // ---- add / delete ------------------------------------------------------
    /// Add a default-size room, cascade-offset so successive rooms don't stack
    /// perfectly, clamped into the grid. Selects it and saves.
    pub fn new_room(&mut self) {
        let id = self.next_id;
        self.next_id += 1;
        let off = (self.rooms.len() as i32 % 4) * 2;
        let x = (2 + off).min(GRID_W - DEFAULT_ROOM_W);
        let y = (2 + off).min(GRID_H - DEFAULT_ROOM_H);
        self.rooms.push(MapRoom {
            id,
            name: format!("ROOM {id:02}"),
            x,
            y,
            w: DEFAULT_ROOM_W,
            h: DEFAULT_ROOM_H,
            cameras: Vec::new(),
        });
        self.selected = self.rooms.len() - 1;
        self.status = format!("Added ROOM {id:02}.");
        save_map(&self.rooms);
    }

    /// Remove the selected room, clamp the cursor, persist.
    pub fn delete_room(&mut self) {
        if self.selected >= self.rooms.len() {
            return;
        }
        let r = self.rooms.remove(self.selected);
        self.clamp();
        self.status = format!("Removed '{}'.", r.name);
        save_map(&self.rooms);
    }

    // ---- geometry edits (Move / Resize) ------------------------------------
    /// Translate the selected room by (dx,dy), clamped so it stays in the grid.
    pub fn nudge(&mut self, dx: i32, dy: i32) {
        if let Some(r) = self.rooms.get_mut(self.selected) {
            r.x = (r.x + dx).clamp(0, GRID_W - r.w);
            r.y = (r.y + dy).clamp(0, GRID_H - r.h);
        }
        self.ensure_visible();
    }

    /// Grow/shrink the selected room; min size 1, kept inside the grid.
    pub fn resize(&mut self, dw: i32, dh: i32) {
        if let Some(r) = self.rooms.get_mut(self.selected) {
            r.w = (r.w + dw).clamp(1, GRID_W - r.x);
            r.h = (r.h + dh).clamp(1, GRID_H - r.y);
        }
    }

    // ---- mode transitions --------------------------------------------------
    pub fn enter_move(&mut self) {
        if !self.rooms.is_empty() {
            self.mode = MapMode::Move;
            self.status = "MOVE \u{2014} arrows reposition, Enter/Esc done".into();
        }
    }

    pub fn enter_resize(&mut self) {
        if !self.rooms.is_empty() {
            self.mode = MapMode::Resize;
            self.status = "RESIZE \u{2014} arrows resize, Enter/Esc done".into();
        }
    }

    /// Commit a Move/Resize edit and persist.
    pub fn commit_geometry(&mut self) {
        self.mode = MapMode::Browse;
        self.status.clear();
        save_map(&self.rooms);
    }

    // ---- view navigation (Pan / zoom — never persisted) --------------------
    pub fn enter_pan(&mut self) {
        self.mode = MapMode::Pan;
        self.status = "PAN \u{2014} arrows move view, +/- zoom, Enter/Esc done".into();
    }

    pub fn exit_pan(&mut self) {
        self.mode = MapMode::Browse;
        self.status.clear();
    }

    // ---- rename (reuses the device handler's push/pop model) ---------------
    pub fn enter_rename(&mut self) {
        if let Some(r) = self.rooms.get(self.selected) {
            self.rename_backup = r.name.clone();
            self.mode = MapMode::Rename;
        }
    }

    pub fn type_char(&mut self, c: char) {
        if self.mode == MapMode::Rename {
            if let Some(r) = self.rooms.get_mut(self.selected) {
                r.name.push(c);
            }
        }
    }

    pub fn backspace(&mut self) {
        if self.mode == MapMode::Rename {
            if let Some(r) = self.rooms.get_mut(self.selected) {
                r.name.pop();
            }
        }
    }

    pub fn commit_rename(&mut self) {
        self.mode = MapMode::Browse;
        self.rename_backup.clear();
        save_map(&self.rooms);
    }

    pub fn cancel_rename(&mut self) {
        let backup = std::mem::take(&mut self.rename_backup);
        if let Some(r) = self.rooms.get_mut(self.selected) {
            r.name = backup;
        }
        self.mode = MapMode::Browse;
    }

    // ---- camera assignment (device data supplied by App) -------------------
    /// Enter the camera picker: prune stale ips so the checklist matches the
    /// live device list, then reset the cursor.
    pub fn enter_cameras(&mut self, valid_ips: &[String]) {
        if self.rooms.is_empty() {
            return;
        }
        self.prune_cameras(valid_ips);
        self.cam_cursor = 0;
        self.mode = MapMode::Cameras;
    }

    pub fn cam_next(&mut self, len: usize) {
        if len > 0 {
            self.cam_cursor = (self.cam_cursor + 1) % len;
        }
    }

    pub fn cam_prev(&mut self, len: usize) {
        if len > 0 {
            self.cam_cursor = (self.cam_cursor + len - 1) % len;
        }
    }

    /// Toggle a camera (by ip) on the selected room. New cameras drop in near the
    /// room centre (cascade-offset so several don't stack). Persists.
    pub fn toggle_camera(&mut self, ip: String) {
        if let Some(r) = self.rooms.get_mut(self.selected) {
            if let Some(pos) = r.cameras.iter().position(|c| c.ip == ip) {
                r.cameras.remove(pos);
                self.status = format!("Unassigned {ip}.");
            } else {
                let n = r.cameras.len() as i32;
                let dx = (r.w / 2 + (n % 3) - 1).clamp(0, (r.w - 1).max(0));
                let dy = (r.h / 2 + (n / 3) % 3 - 1).clamp(0, (r.h - 1).max(0));
                r.cameras.push(RoomCamera { ip: ip.clone(), dx, dy });
                self.status = format!("Assigned {ip}.");
            }
            save_map(&self.rooms);
        }
    }

    /// Nudge the offset of the camera `ip` within the selected room (clamped to
    /// the room). Used by the CamPlace mode arrows.
    pub fn move_camera(&mut self, ip: &str, ddx: i32, ddy: i32) {
        if let Some(r) = self.rooms.get_mut(self.selected) {
            let (w, h) = (r.w, r.h);
            if let Some(c) = r.cameras.iter_mut().find(|c| c.ip == ip) {
                c.dx = (c.dx + ddx).clamp(0, (w - 1).max(0));
                c.dy = (c.dy + ddy).clamp(0, (h - 1).max(0));
            }
        }
    }

    pub fn enter_cam_place(&mut self) {
        if !self.rooms.is_empty() {
            self.mode = MapMode::CamPlace;
            self.status = "POSITION CAMERA \u{2014} arrows move, Enter/Esc done".into();
        }
    }

    /// Commit a camera reposition and return to the picker.
    pub fn commit_cam_place(&mut self) {
        self.mode = MapMode::Cameras;
        self.status.clear();
        save_map(&self.rooms);
    }

    /// Drop cameras whose ip no longer matches a live device, across all rooms.
    pub fn prune_cameras(&mut self, valid_ips: &[String]) {
        let mut changed = false;
        for r in &mut self.rooms {
            let before = r.cameras.len();
            r.cameras.retain(|c| valid_ips.iter().any(|v| v == &c.ip));
            changed |= r.cameras.len() != before;
        }
        if changed {
            save_map(&self.rooms);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::domain::DEFAULT_ZOOM;

    /// Build an in-memory state without touching `map.json`.
    fn state(rooms: Vec<MapRoom>) -> MapState {
        let next_id = rooms.iter().map(|r| r.id).max().map_or(0, |m| m + 1);
        MapState {
            rooms,
            selected: 0,
            mode: MapMode::Browse,
            cam_cursor: 0,
            status: String::new(),
            next_id,
            rename_backup: String::new(),
            view_x: 0,
            view_y: 0,
            zoom: DEFAULT_ZOOM,
        }
    }

    fn room(id: u32, x: i32, y: i32, w: i32, h: i32) -> MapRoom {
        MapRoom { id, name: format!("R{id}"), x, y, w, h, cameras: vec![] }
    }

    #[test]
    fn new_room_increments_id_and_selects() {
        let mut s = state(vec![]);
        s.new_room();
        s.new_room();
        assert_eq!(s.rooms.len(), 2);
        assert_eq!(s.rooms[0].id, 0);
        assert_eq!(s.rooms[1].id, 1);
        assert_eq!(s.next_id, 2);
        assert_eq!(s.selected, 1); // last added
    }

    #[test]
    fn nudge_and_resize_clamp_to_grid() {
        let mut s = state(vec![room(0, GRID_W - DEFAULT_ROOM_W, 0, DEFAULT_ROOM_W, DEFAULT_ROOM_H)]);
        let x0 = s.rooms[0].x;
        s.nudge(5, 0); // already at the right edge → no move
        assert_eq!(s.rooms[0].x, x0);
        s.resize(0, 999); // height clamps to GRID_H - y
        assert_eq!(s.rooms[0].h, GRID_H);
        s.resize(-999, -999); // floors at 1×1
        assert_eq!((s.rooms[0].w, s.rooms[0].h), (1, 1));
    }

    #[test]
    fn delete_clamps_selection() {
        let mut s = state(vec![room(0, 0, 0, 4, 4), room(1, 5, 0, 4, 4), room(2, 10, 0, 4, 4)]);
        s.selected = 2;
        s.delete_room();
        assert_eq!(s.selected, 1);
        s.delete_room();
        s.delete_room();
        assert_eq!(s.selected, 0); // empty, no panic
        assert!(s.rooms.is_empty());
    }

    fn cam_ips(s: &MapState, room: usize) -> Vec<String> {
        s.rooms[room].cameras.iter().map(|c| c.ip.clone()).collect()
    }

    #[test]
    fn toggle_camera_adds_then_removes() {
        let mut s = state(vec![room(0, 0, 0, 4, 4)]);
        s.toggle_camera("10.0.0.1".into());
        s.toggle_camera("10.0.0.2".into());
        assert_eq!(cam_ips(&s, 0), vec!["10.0.0.1", "10.0.0.2"]);
        // new cameras land inside the room
        for c in &s.rooms[0].cameras {
            assert!(c.dx >= 0 && c.dx < 4 && c.dy >= 0 && c.dy < 4);
        }
        s.toggle_camera("10.0.0.1".into());
        assert_eq!(cam_ips(&s, 0), vec!["10.0.0.2"]);
    }

    #[test]
    fn move_camera_clamps_to_room() {
        let mut s = state(vec![room(0, 5, 5, 4, 3)]);
        s.toggle_camera("cam".into());
        s.move_camera("cam", 99, 99); // shove past the far corner
        let c = &s.rooms[0].cameras[0];
        assert_eq!((c.dx, c.dy), (3, 2)); // clamped to w-1, h-1
        s.move_camera("cam", -99, -99);
        let c = &s.rooms[0].cameras[0];
        assert_eq!((c.dx, c.dy), (0, 0));
    }

    #[test]
    fn prune_drops_unknown_ips() {
        let mut s = state(vec![MapRoom {
            id: 0,
            name: "R".into(),
            x: 0,
            y: 0,
            w: 4,
            h: 4,
            cameras: vec![
                RoomCamera { ip: "a".into(), dx: 1, dy: 1 },
                RoomCamera { ip: "b".into(), dx: 2, dy: 2 },
            ],
        }]);
        s.prune_cameras(&["a".into()]);
        assert_eq!(cam_ips(&s, 0), vec!["a"]);
    }

    #[test]
    fn cancel_rename_restores_name() {
        let mut s = state(vec![room(0, 0, 0, 4, 4)]);
        let original = s.rooms[0].name.clone();
        s.enter_rename();
        s.type_char('Z');
        s.cancel_rename();
        assert_eq!(s.rooms[0].name, original);
        assert_eq!(s.mode, MapMode::Browse);
    }

    #[test]
    fn zoom_clamps_and_resizes_viewport() {
        let mut s = state(vec![]);
        let (_, _, w0, h0) = s.viewport();
        assert_eq!((w0, h0), (32, 18)); // default span
        s.zoom_in();
        let (_, _, w1, _) = s.viewport();
        assert!(w1 < w0); // zoomed in shows fewer cells
                          // zooming out past the end clamps at the whole world
        for _ in 0..10 {
            s.zoom_out();
        }
        let (_, _, w2, h2) = s.viewport();
        assert_eq!((w2, h2), (GRID_W, GRID_H));
    }

    #[test]
    fn pan_clamps_to_world() {
        let mut s = state(vec![]);
        for _ in 0..100 {
            s.pan(1, 1);
        }
        let (vx, vy, vw, vh) = s.viewport();
        assert_eq!((vx, vy), (GRID_W - vw, GRID_H - vh));
        for _ in 0..100 {
            s.pan(-1, -1);
        }
        let (vx, vy, _, _) = s.viewport();
        assert_eq!((vx, vy), (0, 0));
    }

    #[test]
    fn ensure_visible_recenters_on_far_room() {
        let mut s = state(vec![room(0, GRID_W - 6, GRID_H - 4, 4, 3)]);
        s.selected = 0;
        s.ensure_visible();
        let (vx, vy, vw, vh) = s.viewport();
        let r = &s.rooms[0];
        assert!(r.x >= vx && r.y >= vy && r.x + r.w <= vx + vw && r.y + r.h <= vy + vh);
    }

    #[test]
    fn fit_frames_all_rooms() {
        let mut s = state(vec![room(0, 0, 0, 4, 4), room(1, GRID_W - 10, GRID_H - 8, 4, 4)]);
        s.fit();
        let (vx, vy, vw, vh) = s.viewport();
        for r in &s.rooms {
            assert!(
                r.x >= vx && r.y >= vy && r.x + r.w <= vx + vw && r.y + r.h <= vy + vh,
                "room not framed by viewport"
            );
        }
    }
}
