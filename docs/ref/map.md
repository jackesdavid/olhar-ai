# Map (floor plan) — `sentinel-app` Map page

The Map is an on-brand, **top-down floor-plan editor rendered with the 3D engine (Threlte /
Three.js) but navigated in flat 2D** — the workflow editor's pan/zoom feel, on a blueprint.
Rooms are labeled *zones* (not walled boxes); **walls** and **furniture/landmark markers** are
drawn primitives; cameras show field-of-view wedges; and the `RecordPresence` workflow node
**pulses live, categorized detections** on it. Built on the "dot-field" identity + Emerald
Guardian theme (see [/impeccable]).

> Status & progress live at the bottom of this file ("Build status"). Keep it current — it's the
> handoff/second-brain for this feature.

## Coordinate model

Grid `(x, y)` maps **1:1 to world `(x, 0, z)`** (the floor is the world XZ plane). An
`OrthographicCamera` looks straight down (`up = (0,0,-1)`), so grid **+x → screen-right**, grid
**+y → screen-down** — it reads exactly like a 2D plan. Navigation is pan (camera X/Z) + zoom
(`camera.zoom`); **no orbit/rotation**. All screen↔world and framing math is in
`lib/map/view3d.ts` (`Viewport`): `screenToWorld`, `panByPixels`, `zoomAt`, `zoomBy`, `fit`,
`apply(cam)`.

## File layout (vertical decomposition — keep it this way)

| Path | Role |
|---|---|
| `routes/map/+page.svelte` | Thin shell: owns `MapStore` + `Viewport` + `ToolState` + `PresenceStore`; composes Toolbar / canvas / EditorPanel; zoom+legend overlays; keydown wiring. |
| `routes/map/components/Toolbar.svelte` | Title, tool palette (Select/Wall/Marker), undo/redo, Save. |
| `routes/map/components/EditorPanel.svelte` | Right column; switches props by selection (wall→marker→room). |
| `…/RoomList, RoomForm, CameraAssign` | Room zone list + name/x/y/w/h + camera assign (with FOV rotation). |
| `…/WallProps, MarkerProps, MarkerPicker` | Selected-wall thickness; selected-marker icon/rot/scale/label; icon library. |
| `lib/map/store.svelte.ts` | `MapStore`: rooms+walls+markers, tagged selection (`room\|wall\|marker`), per-kind CRUD, undo/redo, load/save the doc. |
| `lib/map/tools.ts` | `ToolState` (mode/draft/hover) + snapping (`snapToGrid`, `snapToVertex`, `orthoLock` → 45°, `resolvePoint`). |
| `lib/map/pick.ts` | JS hit-testing (markers→walls→rooms) + wall-endpoint grab. |
| `lib/map/history.ts` | `History`: undo/redo stack over deep-cloned `MapDoc`. |
| `lib/map/keyboard.ts` | `handleMapKey`: arrows nudge, Shift+arrows grow/shrink room (Alt=shrink), Tab cycle, V/W/M, Esc, Delete, F, ⌘Z/⇧⌘Z. |
| `lib/map/view3d.ts` | `Viewport` (ortho 2D nav math). |
| `lib/map/icons.ts` | Marker icon library (SVG strings; furniture/landmarks incl. door-swing + arc). |
| `lib/map/categories.ts` | Presence `Category` → `{color,label}` (single source of truth: pulse + legend). |
| `lib/map/presence.svelte.ts` | `PresenceStore`: SSE subscribe + place + auto-decay pulses. |
| `lib/three/map/MapScene.svelte` | Scene root: ortho camera, lighting (ambient + key dir light w/ soft shadows + emerald rim), tool-aware pointer handling, composes layers. |
| `…/GroundGrid` | Dot-field floor (tiled emerald dots + radial glow). |
| `…/RoomMesh` | One zone: floor fill + boundary outline (no walls) + flat label + cameras. |
| `…/CameraMarker` | Camera glyph + translucent FOV wedge (yaw from `RoomCamera.rot`). |
| `…/WallLayer + WallSegment` | Walls as boxes (thickness → width+height: thin=line, thick=raised solid w/ shadow) + endpoint handles. |
| `…/MarkerLayer + MarkerGlyph` | Markers: SVG → `ShapeGeometry` (SVGLoader), emerald glyph, yaw/scale/label. |
| `…/DrawPreview` | Rubber-band wall/marker placement preview. |
| `…/PresencePulse` | The "ping": concentric rings + centre dot (useTask-animated), category-tinted. |

## Data model & persistence

- `MapRoom { id,name,x,y,w,h,cameras[] }` — a **labeled zone** (axis-aligned, no walls implied →
  open spaces like a backyard/open garage just have none). `RoomCamera { ip,dx,dy,rot? }` (`rot`
  = FOV yaw°, additive/defaulted).
- `MapWall { id,x1,y1,x2,y2,thickness }` — one primitive; thickness = line ↔ solid wall.
- `MapMarker { id,icon,x,y,rot,scale,label? }` — furniture/landmark glyph.
- `MapDoc { rooms, walls, markers }` — the whole plan, exchanged with **`GET/PUT /map`** in one
  replace-all round-trip (`data.loadMap`/`saveMap`).
- Backend: `crates/app-core/src/map/{mod,repo}.rs` — `load_doc`/`save_doc`; tables `map_rooms`
  (existing) + `map_walls` + `map_markers` (migration **`0019_map_geometry`**), one JSONB row each,
  tenant-scoped. Control handlers in `sentinel-control/src/domain/map.rs`.

## Interaction

- **Select** — click picks topmost (JS hit-test); drag moves; drag a wall endpoint reshapes; empty
  drag pans; empty click deselects.
- **Wall** — click start → click end, chained; **Shift snaps to 45°/90°** (clean diagonals); grid +
  vertex snap so walls join. Thicken a thin wall in WallProps to make it solid.
- **Marker** — pick an icon, click the floor to drop; edit icon/rot/scale/label in MarkerProps.
- Undo/redo is coarse (checkpoints on discrete actions + once per drag gesture).

## Presence (live, categorized)

- **`RecordPresence`** node (`NodeKind::RecordPresence`, category "Presence") —
  `crates/app-core/src/workflows/{domain,node}.rs`. Inputs `room/category/label/camera` (override
  config); resolves room→`room_id` like `CreateAlert`. Categories: person, known_person, car, cat,
  dog, unknown.
- Persists via `crates/app-core/src/presence/{mod,repo}.rs` → table `presence` (migration
  **`0020_presence`**, RFC3339 text, per-tenant id). Emits `ToolUiEvent::Presence`
  (`agent/tools.rs`) → `sentinel-flow/src/events.rs` publishes a `presence` SSE line → control
  relay → browser.
- Frontend: `PresenceStore` subscribes via `onEvent("presence")` (`lib/data.ts`), places the pulse
  **ahead in the detecting camera's FOV** (resolves camera id→ip + room rotation; else room centre),
  and expires after the event's **`duration_secs`** — persisted on the node + presence row (migration
  `0021_presence_duration`, fallback 8s). Baseline replays the last 30s from `GET /map/presence`
  (`data.loadPresence`). Pulse colour/legend from `categories.ts`. **Debug:** `[presence]
  pulse/dropped` `console` lines in `presence.svelte.ts` — remove once live presence is confirmed.
- Camera **FOV is a wall-occluded line-of-sight polygon** (`lib/map/visibility.ts`): rays across the
  cone clipped at the nearest wall or `range`. Cameras render in world-space `CameraLayer` (out of the
  rotated room group); per-camera `fov`/`range`/`rot`. The ortho camera is driven by ONE `$size`
  effect in `MapScene` (re-applies on every resize), and zoom/pan persist to localStorage
  (`map.viewport`). Workflow node config **saves as you type** (debounced).

## Conventions / gotchas

- The node **catalog auto-derives** from `NodeKind::ALL` (now 23) — no frontend palette edit.
  `ToolUiEvent::Presence` in `agent/tools.rs` is the one required `tools.rs` touch (the UI event
  channel, not the catalog).
- Timestamps are **RFC3339 text** (never `timestamptz`). Migrations are additive; **next number is
  0022** (0019 map geometry, 0020 presence, 0021 presence `duration_secs`). Note: `duration_secs` is
  `integer`/INT4 → decode/bind as **`i32`** in `presence/repo.rs`.
- House style holds: helpers over template logic, no nested ternaries, emerald `--primary`, dot-field
  identity, web-only.
- The scene's default frameloop renders continuously (needed for the pulse animation).

## Build status

- **Branch `feat/map-redesign`** (off `main`), **deployed to prod** incrementally (`592a3da`→
  `c885862`). `svelte-check` + web build clean (only the 7 pre-existing `sensor-field`/`workflows`
  errors); `cargo check` clean (app-core/control/flow). Not yet merged to `new-ui`.
  1. Decompose + blueprint render · 2. Wall + marker drawing + undo/redo · 3. Keyboard ·
  4. Presence backend · 5. Live pulse + legend · **6. Polish:** full-bleed + canvas overlays;
  Properties|Explorer tabs + shortcuts overlay; top-down **wireframe** markers + outlined walls;
  per-icon anchor + marker→wall snap; rotate rooms/walls/markers; camera **FOV width/length** +
  editable position; **FOV = wall-occluded line-of-sight** (Option B); **zoom fix + persisted
  zoom/pan**; presence dropdowns + FOV-ahead + **persisted `duration_secs`** + save-as-you-type.
- **Pending:** (1) confirm `c885862` (i32 fix) deploy finished; (2) **verify presence live** (run a
  workflow, console shows `[presence] pulse …`); (3) **remove the `[presence]` debug `console` lines**
  once confirmed; (4) release: web version bump + merge to `new-ui`.
- **Backlog:** presence history/heatmap, per-camera FOV tuning beyond yaw, multi-floor maps,
  doors/windows + curved walls + auto-room-detect, optional true 3D wireframe models for hero
  landmarks.

[/impeccable]: ../../.claude/skills/impeccable/SKILL.md
