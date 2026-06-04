# Architecture Notes — Floor-Plan Map

## Components

- `src/map/domain.rs` — `MapRoom` (x,y,w,h,cameras), `RoomCamera` (ip,dx,dy), `MapState`
  (rooms, selected, mode, cam_cursor, view_x/y, zoom, next_id, status), pan/zoom logic,
  `load`/`save` (`map.json` + `mapview.json`).
- `src/map/handler.rs` — selection, geometry edits (move/resize), rename, camera placement,
  mode-per-keymap.
- `src/map/view.rs` — floor-plan canvas (Braille grid, y-flip, `CanvasRect` rooms, camera
  markers), context/camera-picker panels, empty-state.
- `src/ui/` — `CanvasRect`/`CanvasLine`, theme, chrome.

## APIs

None external. Pure local state + JSON persistence.

## Data Flow

Operator edits in a mode → `handler` mutates `MapState` → `view` redraws the canvas each tick
→ changes persist to `map.json` (rooms) and `mapview.json` (viewport). Tests skip disk via
`if cfg!(test) { return }` and an in-memory constructor.

## Important Decisions

- Index-based selection (Up/Down over rooms) — no coordinate hit-testing needed.
- Viewport persisted separately from rooms so layout and view are independent concerns.

## Risks

- Coordinate math (y-down, zoom levels) — mitigated by unit tests in `domain.rs`.
