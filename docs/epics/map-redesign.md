# Epic — Map redesign (top-down 3D blueprint editor + live presence)

**Status:** in progress (deployed to prod; verifying presence-live + cleanup before release) ·
**Branch:** `feat/map-redesign` · **Ref:** [docs/ref/map.md](../ref/map.md) · **Board:** card "Map
redesign" in `docs/Task Board.md`

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
- [x] **Deploy to production** — shipped incrementally via `task cloud-deploy` (applied `0019`–`0021`
  on control/flow boot). Last: `c885862`.
- [x] **Phase 6 — Polish (this session).** Full-bleed layout; canvas overlays (undo/redo, tools,
  zoom); collapsible Properties|Explorer tabs + shortcuts overlay; top-down wireframe markers +
  outlined walls; per-icon anchor + marker→wall snap; rotate rooms/walls/markers; camera FOV
  width/length + editable position; **camera FOV as wall-occluded line-of-sight** (Option B); zoom
  fix + persisted zoom/pan; presence dropdowns, FOV-ahead pulse, **persisted `duration_secs`
  (migration 0021)**, save-as-you-type config, removed `timeout_secs`.
- [ ] **Verify presence live** — run a workflow with the console open; expect `[presence] pulse …`
  (no `dropped`). If `dropped`, the room/camera doesn't match a saved room.
- [ ] **Remove `[presence]` debug `console` lines** (`lib/map/presence.svelte.ts`) once confirmed.
- [ ] **Release** (when asked): bump web version (`package.json` + `+layout.svelte` footer); merge
  `feat/map-redesign` → `new-ui`.

## Backlog (out of scope for this epic)

Presence history/heatmap, per-camera FOV tuning beyond yaw, multi-floor maps, doors/windows +
curved walls + auto-room-detect, optional true 3D wireframe models for hero landmarks.
