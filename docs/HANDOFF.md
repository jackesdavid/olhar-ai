# Handoff log

Rolling session memory (newest first). Maintained via the `/handoff` skill. Each entry: what
changed, branch/commits, state, what's pending, blockers. Deep per-area detail lives in
`docs/ref/*.md`.

---

## 2026-06-19 — GPU box setup: Tailscale + SSH + sidecar build (CUDA mismatch blocked)

- **Branch:** worktree `scalable-bouncing-hamming` off `new-ui`; plan note committed to `feat/map-redesign` via cherry-pick
- **Commits:** `d966e5f` (face-recognition-next-steps plan + board card, on `feat/map-redesign`)
- **Done:**
  - **Face recognition plan note:** `docs/plans/face-recognition-next-steps.md` — full state snapshot + 4-phase next-steps (DetectFaces node, live-feed triggers, GPU box upgrade, hardening). Board card added to ToDo.
  - **GPU box access:** Tailscale on Bazzite Linux box (`100.84.250.126`). SSH alias `gpu-box` in `~/.ssh/config`, key `~/.ssh/id_rsa`. NOPASSWD sudo for `/usr/bin/podman` via `/etc/sudoers.d/sentinel-ai`.
  - **GPU confirmed:** RTX 2060 SUPER, driver 595.71.05 (CUDA 13.2 capable), CDI config present (`/etc/cdi/nvidia.yaml`), GPU passthrough works with `sudo podman`.
  - **Sidecar image built:** `localhost/sentinel-ai:gpu` (8.4 GB) on the box; container `sentinel-ai` running on port 8000. `Dockerfile.gpu` on box patches `FROM` with `docker.io/` prefix (Podman Fedora policy).
- **State:** Sidecar running but on **CPUExecutionProvider** — GPU not active. `torch.cuda.is_available()` = True; problem is onnxruntime-gpu CUDA version.
- **Blocker — CUDA version mismatch:**
  PyPI onnxruntime-gpu skips CUDA 12: ≤1.17.x links `.so.11`, ≥1.19.x links `.so.13`. Dockerfile base (`pytorch:2.4.1-cuda12.4`) ships `.so.12`. CUDA 12.4 apt packages installed in running container. **Try `onnxruntime-gpu==1.18.1` next** — likely the CUDA 12 sweet spot. Fallback: direct wheel from ONNX Runtime GitHub releases.
- **Pending / next:**
  1. `ssh gpu-box "sudo podman exec sentinel-ai pip install onnxruntime-gpu==1.18.1"` → `ldd libonnxruntime_providers_cuda.so | grep "not found"` → if `.so.12`, update LD_LIBRARY_PATH to CUDA 12.4 apt paths + restart
  2. If 1.18.1 still wrong CUDA, get wheel from https://github.com/microsoft/onnxruntime/releases
  3. Once GPU confirmed: update `Dockerfile.gpu`, rebuild image, smoke-test with a real face image
  4. Wire into VPS: `sentinel.env` → `AI_SERVICE_URL=http://100.84.250.126:8000/runsync` + `extra_hosts` in sentinel-control service
- **Blockers:** onnxruntime CUDA gap — autonomous if 1.18.1 works; needs user if Dockerfile rebuild required
- **Refs:** `docs/ref/ai-sidecar.md` (new this session), `docs/plans/face-recognition-next-steps.md`

---

## 2026-06-19 — Workflow nodes: ObjectDetection + comparison nodes + SSE animation fix

- **Branch:** `feat/map-redesign` (all workflow work on this branch); pushed via `task cloud-deploy`.
- **Commits:** `14ef865` (new nodes), `c8c1f78` (SSE animation fix).
- **Done:**
  - **Generic Object Detection node** (`object_detection`): any COCO class via dropdown or wired `class` Text input; outputs `count`, `confidence`, `detections` (PersonArray), `ok`. Wired class pin grays out the dropdown.
  - **Break Detection node** (`break_detection`): splits a single detection into `bbox` (NumArray) + `confidence`.
  - **Missing comparison/logic nodes:** `Or`, `Not`, `TextCompare` (eq/ne/contains/starts_with/ends_with) — all pure pull-based with inline expression labels in the canvas.
  - **Workflow node animations fixed:** `events_stream` in sentinel-control returned 502 when `REDIS_URL` was unset — the promised single-pod fallback was never built. Fixed: (1) `GET /events` SSE on sentinel-flow streams the in-process bus; (2) sentinel-control proxies it and filters by tenant when Redis is absent. Redis path (production) unchanged.
  - **`.env.example`**: documented `REDIS_URL` as optional with fallback note.
  - **Board:** "Node run animation lags" → Done.
  - **Deployed:** `task cloud-deploy -- sentinel-flow sentinel-control` in flight at handoff time.
- **State:**
  - `cargo check` clean for both crates before commit. `svelte-check` not re-run (no frontend changes this session).
  - Deploy was still building when handoff was saved — verify containers restarted on VPS.
- **Pending / next:**
  1. Verify animations work in browser after deploy settles.
  2. Move "Generic Object Detection" + "Missing comparison nodes" board cards → Done.
  3. RecordPresence room picker bug (Review column) still open.
  4. Map: wall depth card still Review; remove `[presence]` debug console lines.
  5. Release: bump web version + merge `feat/map-redesign` → `new-ui`.
- **Blockers:** none.
- **Refs:** `sentinel-flow/src/http.rs:171` (new `/events`), `sentinel-control/src/domain/workflows.rs:353` (`events_stream`), `docs/ref/workflows.md`, `docs/workflow-nodes.md`.

---

## 2026-06-18 — Map 3D wall depth polish + no-Docker deploy path

- **Branch:** `feat/map-redesign` (off `main`); pushed. Not merged to `new-ui`.
- **Commits:** `022dbb5`, `9526caa`, `c28f792` (this session, on top of `c885862`).
- **Done (all deployed to prod via no-Docker path):**
  - **Wall height:** uniform `0.8` for all solid walls regardless of thickness — CAD/AutoCAD feel; wireframe stays `0.04` (`WallSegment.svelte:24`).
  - **Wall material:** `roughness 0.8→0.65`, `metalness 0.06` added — painted-plaster/concrete surface feel (`WallSegment.svelte:39`).
  - **Lighting:** ambient `0.85→0.5`, key light `1.15→1.7`, shadow map `2048→4096`, frustum `±120→±80`, sun inclined `[40,70,24]→[90,38,20]` — deep one-sided shadows from the right (`MapScene.svelte`).
  - **Vertex snap:** `0.4→0.65` — wider magnetic zone so wall endpoints connect cleanly (`tools.svelte.ts`).
  - **Board:** "Map: solid walls taller, richer material, deeper shadows" card created (ToDo → In Progress); plan at `docs/plans/map-walls-3d-depth.md`.
  - **No-Docker deploy path confirmed:** `npm run build` → `scp` tarball to VPS → `docker cp` into running `sentinel-app` container → `nginx -s reload`. Used twice this session (Docker Desktop was down). HTTP 200 confirmed via public URL.
- **State:** web build clean (same 7 pre-existing warnings); not cargo-checked this session (no backend changes). Deployed to prod; `app.thesentinels.online` returning 200.
- **Pending / next:**
  1. Run `task cloud-deploy -- sentinel-app` once Docker Desktop is back (proper image rebuild so the no-Docker inject is superseded by a clean image).
  2. Confirm `c885862` (i32 presence fix) deploy is still live.
  3. Verify presence live in browser console (`[presence] pulse …`).
  4. Remove `[presence]` debug console lines in `lib/map/presence.svelte.ts` once confirmed.
  5. Release: bump web version + merge `feat/map-redesign` → `new-ui`.
- **Blockers:** Docker Desktop needs a restart before `task cloud-deploy` can run.
- **Refs:** `docs/ref/map.md` (Build status §7), `docs/plans/map-walls-3d-depth.md`.

---

## 2026-06-17 — Map polish + presence end-to-end + edge install fix (deployed)

- **Branch:** `feat/map-redesign` (off `main`); pushed. **Not** merged to `new-ui`.
- **Commits:** `592a3da` → `c885862` (this session, on top of the earlier `321dc70…b39bcbf`).
- **Done (all deployed to prod incrementally):**
  - **Edge install:** `install.sh` self-heals the macOS `tailscaled` system daemon on both full
    install and `--update` (idempotent `ensure_tailscaled_macos`, waits + warns) — fixed OfficeBox
    "offline on tailnet". Moved the box install command from a modal to its own view
    (`/settings/boxes/install`, `lib/box-install.svelte.ts`).
  - **Map crash:** `lib/map/{history,tools,view3d}` were plain `.ts` using runes → `$state is not
    defined` at runtime; renamed to `.svelte.ts` (+ import paths).
  - **Map UX:** full-bleed layout (added `/map` to layout `FULL_BLEED`); undo/redo overlay
    top-left, tool palette center-right, zoom top-right; collapsible right panel with icon tabs
    **Properties | Explorer** (`SidePanel`, `ExplorerPanel`, `PropertiesPanel`); shortcuts overlay
    (`?` / keyboard button).
  - **Wireframe styling:** furniture markers redrawn as top-down plan symbols rendered as emerald
    outline + faint fill (line loops); walls get a bright emerald edge outline.
  - **Alignment:** per-icon `anchor` (door = hinge) + markers snap onto the nearest wall line.
  - **Rotation:** rooms (about centre, slider + `[`/`]`), walls (about midpoint, buttons + `[`/`]`),
    markers; camera **FOV width + length** per-camera; editable camera dx/dy after assign.
  - **Camera FOV = wall-occluded line-of-sight polygon** (Option B): `lib/map/visibility.ts`,
    world-space `CameraLayer` (cameras moved out of the rotated room group).
  - **Zoom fix:** single `$size`-driven effect re-applies the ortho camera on every resize (start +
    panel toggle no longer reset the view); **camera zoom/pan persisted** to localStorage.
  - **Presence end-to-end working:** room + category **dropdowns** on the node; camera id→ip resolve;
    pulse placed **ahead in the FOV**; configurable **`duration_secs`** now **persisted** (migration
    `0021_presence_duration`) so live + replay honor it; removed `timeout_secs` field + the 6s
    fallback; workflow config **saves as you type** (debounced, no Enter). Baseline replays last 30s.
- **State:** `svelte-check` + web build clean (only the 7 pre-existing `sensor-field`/`workflows`
  errors); `cargo check` clean for app-core/control/flow. Deployed throughout via `task cloud-deploy`
  (app many times; control+flow for presence/duration). **In flight at handoff:** control+flow deploy
  of `c885862` (the i32 fix) — building when this was written; verify it finished.
- **Pending / next:** (1) confirm `c885862` deploy landed (`OK: deployed …`, app 200); (2) **verify
  presence live** — run the workflow with console open, expect `[presence] pulse …` (no `dropped`);
  (3) **remove the `[presence]` debug `console` lines** in `lib/map/presence.svelte.ts` once
  confirmed; (4) release: web version bump + merge `feat/map-redesign` → `new-ui`.
- **Blockers:** none code-side. Transient GHCR/Docker-Hub TLS/network hiccups during deploys (retry
  worked). SSH to prod for debugging is **not authorized** (classifier-blocked) — diagnose from code.
- **Refs:** `docs/ref/map.md` (Build status), `docs/ref/edge.md`, `docs/epics/map-redesign.md`.

---

## 2026-06-18 — Map 3D wall depth polish + no-Docker deploy path

- **Branch:** `feat/map-redesign` (off `main`); pushed. Not merged to `new-ui`.
- **Commits:** `022dbb5`, `9526caa`, `c28f792` (this session, on top of `c885862`).
- **Done (all deployed to prod via no-Docker path):**
  - **Wall height:** uniform `0.8` for all solid walls regardless of thickness — CAD/AutoCAD feel; wireframe stays `0.04` (`WallSegment.svelte:24`).
  - **Wall material:** `roughness 0.8→0.65`, `metalness 0.06` added — painted-plaster/concrete surface feel (`WallSegment.svelte:39`).
  - **Lighting:** ambient `0.85→0.5`, key light `1.15→1.7`, shadow map `2048→4096`, frustum `±120→±80`, sun inclined `[40,70,24]→[90,38,20]` — deep one-sided shadows from the right (`MapScene.svelte`).
  - **Vertex snap:** `0.4→0.65` — wider magnetic zone so wall endpoints connect cleanly (`tools.svelte.ts`).
  - **Board:** "Map: solid walls taller, richer material, deeper shadows" card created (ToDo → In Progress); plan at `docs/plans/map-walls-3d-depth.md`.
  - **No-Docker deploy path confirmed:** `npm run build` → `scp` tarball to VPS → `docker cp` into running `sentinel-app` container → `nginx -s reload`. Used twice this session (Docker Desktop was down). HTTP 200 confirmed via public URL.
- **State:** web build clean (same 7 pre-existing warnings); not cargo-checked this session (no backend changes). Deployed to prod; `app.thesentinels.online` returning 200.
- **Pending / next:**
  1. Run `task cloud-deploy -- sentinel-app` once Docker Desktop is back (proper image rebuild so the no-Docker inject is superseded by a clean image).
  2. Confirm `c885862` (i32 presence fix) deploy is still live.
  3. Verify presence live in browser console (`[presence] pulse …`).
  4. Remove `[presence]` debug console lines in `lib/map/presence.svelte.ts` once confirmed.
  5. Release: bump web version + merge `feat/map-redesign` → `new-ui`.
- **Blockers:** Docker Desktop needs a restart before `task cloud-deploy` can run.
- **Refs:** `docs/ref/map.md` (Build status §7), `docs/plans/map-walls-3d-depth.md`.

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
