---
name: map-feature
description: "Second brain for the Sentinel Map (floor-plan) feature — the top-down 3D blueprint editor: rooms as zones, drawn walls + SVG furniture/landmark markers, camera FOV wedges, keyboard/undo, and the RecordPresence node + live categorized presence pulse. Invoke when building, fixing, reviewing, deploying, or resuming any Map work, or when the user says 'continue the map', 'the floor plan', 'presence pulse', '/map-feature'. Loads the architecture, conventions, file layout, and current build status so work continues without re-discovery."
user-invocable: true
argument-hint: "[status | continue | deploy | <what to work on>]"
---

# Map feature — second brain

The Map is the top-down floor-plan editor (Threlte/Three.js rendered, 2D-navigated) in
`sentinel-app`. This skill is the entry point for continuing it without rediscovering the design.

## Do this first

1. **Read [`docs/ref/map.md`](../../../docs/ref/map.md)** — the source of truth: coordinate model,
   the vertical file layout (every file + role), data model + persistence, interaction (tools,
   snapping, keyboard), the presence pipeline, conventions/gotchas, and the **Build status**
   section (what's done, what's pending, the active branch, the backlog).
2. **Check the branch**: work happens on **`feat/map-redesign`**. `git log --oneline -8` shows the
   per-phase commits. Confirm the working tree is clean before starting.
3. If the task is unclear, re-read "Build status" in the doc and pick the next pending item, or ask.

## How to work on it

- **Respect the vertical decomposition.** New behavior goes in the right small file
  (`lib/map/*` for logic, `lib/three/map/*` for scene layers, `routes/map/components/*` for panel
  UI). Don't grow `+page.svelte` or `MapScene.svelte` into monoliths.
- **Conventions** (also in CLAUDE.md): helpers over template logic, no nested ternaries, emerald
  `--primary` + dot-field identity, RFC3339 text timestamps, additive migrations (next is 0021),
  no Claude attribution in commits. The node catalog auto-derives from `NodeKind::ALL`.
- **Verify before pushing**: `npm run check` then `npm run build` in `sentinel-app` (only the 7
  pre-existing `sensor-field`/`workflows` errors are acceptable); `cargo check -p app-core -p
  sentinel-control -p sentinel-flow` for backend changes.
- **Push per increment** to `feat/map-redesign` so the user can test, and **update the "Build
  status" section of `docs/ref/map.md`** in the same change — that section is the living progress
  log / second brain. Keep it honest (done vs pending vs not-done).

## Continuing / deploying

- **Deploy to production** = `task cloud-deploy` (local Docker build → GHCR → VPS; applies
  migrations on control/flow boot). Needs Docker Desktop's engine actually up (not just the app —
  it may wait on a UI password), `gh` logged in as `jackesdavid`, and the VPS key. See
  [`docs/ref/deploy.md`](../../../docs/ref/deploy.md). Confirm with the user before deploying.
- **Release** (separate, only when asked): bump web version (`sentinel-app/package.json` + footer
  in `routes/+layout.svelte`); merge `feat/map-redesign` → `new-ui`.
- For visual/design polish, also invoke **/impeccable**.

## Scope guard

The edge is the only camera-touching component; the Map is cloud-only and never persists camera
credentials. Presence is cloud-only. Don't add cloud credential persistence.
