# Spec: 018 — Web migration: Map (3D floor plan)

> **Status: DONE (code) — web migration unit (M2).** cargo check + npm build clean (4 map core tests); ratatui src/ untouched. 3D view reuses the macOS-verified 0c spike pattern; the /map page itself is runtime-checked by the operator.

## Goal

The operator lays out rooms on a 3D floor plan, places cameras, and jumps from a room's camera
straight to its live feed.

## User Value

Spatial awareness — which camera covers which room — and a one-click path room → live. Ties
Devices + Live + Map together. The wow feature (validated as the 0c Threlte spike).

## Requirements

- Copy `src/map` domain (`MapRoom`, `RoomCamera`) + persistence into `app/crates/core`
  (drop the ratatui `MapState`/canvas editor; persistence path-parameterized → app-data `map.json`).
- Commands: `load_map() -> Vec<MapRoom>`, `save_map(rooms)`.
- Frontend `/map`: a **Threlte (Three.js) 3D scene** of the rooms (real `map.json`) + camera
  markers, OrbitControls, click-to-select (reuse the 0c spike pattern), with shadcn chrome.
- **Editor** (side panel): add / rename / delete rooms; edit x/y/w/h; assign cameras to a room
  (pick a saved device + dx/dy) / unassign; Save → `save_map`.
- **Camera → Live**: clicking a room's camera (marker or panel) navigates to `/live` preselecting
  that device.

## Acceptance Criteria

- [ ] `load_map`/`save_map` round-trip rooms (app-data `map.json`), rendered in 3D.
- [ ] Add/rename/delete a room and assign a camera; Save persists; reload shows it.
- [ ] Clicking a camera opens Live for that device.
- [ ] `cargo check` + `npm run build` clean; ratatui `src/` untouched.

## Dependencies

- 014 Devices (camera list for assignment), 015 Live (the link target), M0.

## Risks

- 3D editing UX — keep edits in a side panel (inputs), not 3D drag-handles, for v1.

## Notes

Threlte deps (`@threlte/core`, `@threlte/extras`, `three`) added to `app/`. Reuse
`web-spike-0a/src/lib/three/Scene.svelte` as the scene reference.
