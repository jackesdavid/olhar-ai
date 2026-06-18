# Map Redesign 2 — Implementation Plan

**Ref:** docs/ref/map.md · **Branch:** feat/map-redesign
**Goal:** Make wall **style** (wireframe vs solid) a property distinct from thickness, and shift the map's look toward a polished architectural floor plan (cool blue-slate + near-white geometry), reserving emerald for live/interactive accents.

## Context

The Map page (top-down 3D blueprint editor, Three.js→Threlte) works, but:

1. **Wall "style" is faked from thickness.** A wall only has `thickness`; the UI label flips between "Line" and "Solid wall" at a threshold (`t >= 0.22`). We want **two distinct properties**: a `style` dropdown (wireframe vs solid) *and* an independent thickness slider.
2. **The aesthetic doesn't read as an architectural floor plan.** Reference: cool blue-slate background, crisp near-white walls + furniture line-art, grey uppercase room labels, soft drop shadows. Today it's emerald-on-dark-green.

## Decisions (confirmed with user)

- **Wall style = outline vs filled.** `wireframe` → edge lines only (flat blueprint line, no raised fill/shadow). `solid` → current raised filled mesh + shadow. Thickness stays an independent width control in both modes.
- **Palette = architectural neutral.** Blue-slate bg, near-white walls/furniture, grey uppercase labels. Emerald kept ONLY for selection, camera FOV, presence pulses.

## Phase A — Wall `style` as a distinct property

**Data model** (add `style: "solid" | "wireframe"`, default `"solid"`):
- `sentinel-app/src/lib/types.ts:163` — add `style?: "solid" | "wireframe"` to `MapWall`.
- `crates/app-core/src/map/mod.rs:115` — add `#[serde(default = "default_style")]` field + `default_style()` → solid. Walls persist as serialized JSON in `map_walls.data` (jsonb), so **no migration** — serde default covers old rows, same pattern as `thickness`.

**Rendering** — `sentinel-app/src/lib/three/map/WallSegment.svelte:18-36`:
- Keep height/depth from `thickness` for `solid`.
- `style === "wireframe"` → skip the raised `T.Mesh` fill (or near-zero height + `castShadow={false}`), keep only the `T.LineSegments` outline; width still tracks `thickness`.

**UI** — `sentinel-app/src/routes/map/components/WallProps.svelte`:
- Add a **Style** dropdown (shadcn `Select`) above the thickness slider: Solid / Wireframe → `store.updateWall(wall.id, { style })` (store already takes `Partial<MapWall>` at `lib/map/store.svelte.ts:296`).
- **Remove the threshold `kindLabel()`** (`:15`) — style is explicit now; thickness label just shows width (thin↔thick), not "Line/Solid wall".
- New-wall defaults: set `style: "solid"` where walls are created.

## Phase B — Architectural-neutral visual pass

Swap geometry palette emerald→neutral; keep emerald only for live/interactive bits. Centralize colors in a new `lib/map/palette.ts` (named constants, no scattered hex literals).

- **Background / floor** — `GroundGrid.svelte:30-48` + `textures.ts`: floor base `#16201d` → cool blue-slate (`~#2b353d`); soften/neutralize the emerald dot-field + radial glow. Check canvas clear color in `MapScene.svelte`.
- **Walls** — `WallSegment.svelte`: unselected fill `#22332e` → near-white/slate, edge `#34d399` → `#e8eef0`. Keep **selected** emerald (`#6ee7b7`).
- **Rooms** — `RoomMesh.svelte:35-68`: neutralize unselected fill/stroke to cool grey; labels `#7c9a8f` → `~#8a97a0` and **uppercase** via a helper (no logic in markup). Keep selected emerald.
- **Furniture markers** — `MarkerGlyph.svelte:66-78`: unselected line `#34d399` → near-white; keep selected emerald. (Icons in `lib/map/icons.ts` already blueprint-style.)
- **Soft depth** — solid walls already `castShadow` w/ PCFSoftShadowMap (`MapScene.svelte:59`); tune `DirectionalLight` for a soft shadow like the reference; optional faint contact-shadow under solid walls.
- **Keep emerald (don't touch):** `CameraMarker.svelte` FOV + body, `PresencePulse.svelte`, selection highlights, `DrawPreview.svelte`.

## House-style guardrails
- No nested ternaries / logic in markup — named helpers returning string/bool (`wallEdgeColor(selected)`, `roomLabel(name)`).
- Web-only shadcn (Select); no `window.prompt/confirm`.
- Emerald `--primary` stays the app theme; only the *map geometry* goes neutral.

## Verification
1. `task dev`, open `/map`. Draw a wall → defaults **solid** (filled + shadow).
2. Select → **Style** dropdown + **Thickness** slider independent: switch to **Wireframe** (outline only), drag thickness (width changes, style stays). Back to **Solid** → filled.
3. Reload → style persists; old walls (no `style`) load as solid (serde default).
4. Visual: bg blue-slate, walls/furniture near-white, labels grey uppercase; selection / camera FOV / presence pulses remain emerald.
5. `task check` + `task cargo`.

## Out of scope (v1)
- Per-room floor textures (tile/carpet), dimensional annotations, multi-floor elevation.
