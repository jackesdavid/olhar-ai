# Handoff log

Rolling session memory (newest first). Maintained via the `/handoff` skill. Each entry: what
changed, branch/commits, state, what's pending, blockers. Deep per-area detail lives in
`docs/ref/*.md`.

---

## 2026-06-17 — Map redesign (top-down 3D blueprint editor + presence)

- **Branch:** `feat/map-redesign` (off `main`); pushed. **Not** merged to `new-ui`.
- **Commits:** `321dc70` → `b39bcbf` (Phases 1–5 + diagonal-walls + door-swing + docs handoff).
- **Done:** Rebuilt the Map page as a top-down floor-plan editor rendered with the 3D engine,
  navigated in flat 2D. Rooms are labeled zones (no forced walls); wall drawing (one primitive,
  thin=line/thick=solid, Shift snaps 45°/90°); furniture/landmark **markers** as emerald SVG
  glyphs (incl. door-swing + arc); camera FOV wedges; undo/redo; full keyboard; fit/zoom; dot-field
  floor + soft-shadow lighting. New **`RecordPresence`** workflow node persists categorized
  presence (person/known/car/cat/dog/unknown) + pulses it live on the map via SSE. Vertical file
  decomposition (`routes/map/components`, `lib/map`, `lib/three/map`). Migrations `0019_map_geometry`
  + `0020_presence`; `/map` now exchanges a `{rooms,walls,markers}` document; `GET /map/presence`.
- **State:** `npm run check` + `npm run build` clean (only 7 pre-existing `sensor-field`/`workflows`
  errors, untouched); `cargo check` clean for app-core/control/flow. **Not visually QA'd in a
  running app.**
- **Pending / next:** production deploy via `task cloud-deploy` (applies `0019`+`0020` on
  control/flow boot); then visual QA; optional version bump for release + merge to `new-ui`.
- **Blockers:** deploy blocked on Docker Desktop engine not ready (waiting on a macOS UI password);
  re-run `task cloud-deploy` once `docker info` responds.
- **Refs:** `docs/ref/map.md` (full design + Build status), `docs/ref/deploy.md`.
