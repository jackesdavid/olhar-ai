# Epic — Map redesign (top-down 3D blueprint editor + live presence)

**Status:** in progress (build complete; production deploy pending) · **Branch:** `feat/map-redesign`
· **Ref:** [docs/ref/map.md](../ref/map.md) · **Board:** card "Map redesign" in `docs/Task Board.md`

## Goal

Turn the Map into a first-class, on-brand surface: a top-down floor plan rendered with the 3D
engine but navigated in flat 2D — rooms as labeled zones, drawn walls + furniture/landmark markers,
camera field-of-view, full keyboard editing, and a `RecordPresence` workflow node that pulses
categorized detections live. Vertical, maintainable file structure throughout.

## Tasks

- [x] **Phase 1 — Decompose + blueprint render.** Ortho top-down camera, 2D pan/zoom/fit, dot-field
  floor, soft-shadow lighting, rooms as zones (no forced walls), camera FOV wedges; split into
  `routes/map/components`, `lib/map`, `lib/three/map`.
- [x] **Phase 2 — Drawing editor.** Wall primitive (thin line ↔ thick solid, Shift 45°/90°), SVG
  furniture/landmark markers (incl. door-swing + arc), select/drag/reshape, undo/redo, persistence
  (`map_walls` + `map_markers`, `/map` document).
- [x] **Phase 3 — Keyboard.** Arrows nudge, Shift+arrows grow/shrink room (Alt shrink), Tab cycle,
  V/W/M tools, Esc, Delete, F fit, ⌘Z/⇧⌘Z.
- [x] **Phase 4 — Presence backend.** `RecordPresence` node + categories, `presence` table
  (`0020_presence`), `ToolUiEvent::Presence` → flow SSE relay, `GET /map/presence`.
- [x] **Phase 5 — Live pulse.** `PresenceStore` (SSE subscribe + decay), `PresencePulse` ping,
  category legend, `categories.ts` single source of truth.
- [ ] **Deploy to production** — `task cloud-deploy` (applies `0019`+`0020` on control/flow boot).
  *Blocked:* Docker Desktop engine not ready (macOS UI password). Re-run once `docker info` responds.
- [ ] **Visual QA** in a running app (drawing, presence pulse, save round-trip).
- [ ] **Release** (when asked): bump web version (`package.json` + `+layout.svelte` footer); merge
  `feat/map-redesign` → `new-ui`.

## Backlog (out of scope for this epic)

Presence history/heatmap, per-camera FOV tuning beyond yaw, multi-floor maps, doors/windows +
curved walls + auto-room-detect, optional true 3D wireframe models for hero landmarks.
