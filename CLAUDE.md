# Sentinel — project guide for Claude

Sentinel is a multi-tenant SaaS for camera monitoring + automation. Cameras are LAN-bound,
so an on-prem **edge appliance** is the only thing that touches them; everything else is
cloud. The web app is **"Sentinel"** — a calm assistant for home + business (no Alien IP;
plain terms: Recipients / site / zones).

This file is the index. Detailed knowledge lives in focused references — read the one that
matches your task:

- **[Architecture & services](docs/ref/architecture.md)** — the services, ports, data flow,
  multi-tenancy, and how the cloud reaches LAN cameras (the edge's outbound SSE channel).
- **[Deploy & ops](docs/ref/deploy.md)** — how to ship: local build → GHCR → VPS (`task
  cloud-deploy`), the GitHub Actions billing block, the VPS, migrations, versioning, and how
  the edge installer + binary are served.
  discovery + IP auto-reconcile, health reporting, the restart command, and `install.sh`.
- **[Workflows engine](docs/ref/workflows.md)** — `sentinel-flow`: graph execution, loops,
  cross-graph events, run history, camera binding — **and the extraction of the engine itself**
  into [github.com/jackesdavid/graph-engine](https://github.com/jackesdavid/graph-engine)
  (`ggraph-core`, public), shared with a second product. Card #204. (Node catalog:
  [docs/workflow-nodes.md](docs/workflow-nodes.md).)
- **[Tuya integration](docs/ref/tuya.md)** — smart-home provider: one operator Tuya Cloud project
  (HMAC-signed), per-tenant device catalog, the Control/Read Tuya nodes + Integrations page.
- **[Mobile push](docs/ref/push.md)** — the phone Recipient channel: Expo → APNs/FCM, the TWO
  credentials that must both be right (`EXPO_ACCESS_TOKEN` and a Sandbox **&** Production APNs key),
  and the diagnostics — including why a rejected push currently looks like a delivered one.
- **[Map (floor plan)](docs/ref/map.md)** — the Map page: top-down 3D blueprint editor (rooms as
  zones, drawn walls + SVG furniture markers, camera FOV), the `RecordPresence` node + live
  categorized presence pulse, the vertical file layout, and build status. (Active: branch
  `feat/map-redesign`.)
- **[Brand](docs/ref/brand.md)** — the visual identity: the dot-field mark, the Sora sentence-case
  wordmark (`--font-brand`), and the `SentinelLogo` lockup. Monochrome ink; form over colour.
- **[Issue tracker](docs/agents/issue-tracker.md)** — where work is tracked and how the planning
  skills address it. The board is `docs/Task Board.md` (Obsidian Kanban, committed) with detail in
  `docs/plans/`; the doc maps `/wayfinder`'s map/ticket/blocking/frontier operations onto that,
  since a markdown board has no child issues, native blocking or assignees.
- **[Handoff log](docs/HANDOFF.md)** — rolling session memory (newest first): what changed, branch,
  pending, blockers. Save/resume it with the `/handoff` skill.

## Repo layout

| Path | What |
|---|---|
| `sentinel-control/` | Control plane / SaaS backend (Axum, :8790) — auth/RBAC/JWT, tenants, the camera config the edge pulls, box registry, workflow CRUD, event registry, run history + box-log relay |
| `sentinel-flow/` | Always-on workflow engine (Axum + SSE, :8787) — runs graphs, cross-graph events, run history. Multi-tenant (one pod, all tenants) |
| `sentinel-app/` | The web UI (SvelteKit / Svelte 5). Talks to control (API) + gateway (live) |
| `sentinel-landing/`, `landing/` | Marketing landing site |
| `ai/sidecar/` | Face recognition GPU/CPU sidecar (RunPod) |
| `deploy/` | Dockerfiles (`app/`, `landing/`) + `push-local.sh` (local deploy) |
| `legacy/` | Dead Tauri-era code — ignore |

## Build / run / check (Taskfile)

`brew install go-task`, then:
- `task dev` — web dev server; `task control` / `task flow` / `task edge` / `task gateway` — run a service
- `task db-up` — local Postgres (compose); `task build` / `task check` — web build / svelte-check
- `task cargo` — `cargo check` the workspace; `task fmt`
- `task cloud-deploy [-- <services>]` — **legacy local deploy; do NOT use** (see "Ship via GitHub CI"
  below and the [deploy ref](docs/ref/deploy.md))

Backend is Rust (Axum + sqlx, runtime queries — no compile-checked `query!`, so no live DB
needed to build; rustls, no OpenSSL). Frontend is SvelteKit + Svelte 5 runes + shadcn-svelte.

## Conventions that bite if ignored

- **The edge is the only camera-touching component.** Camera **credentials + RTSP URLs live
  only on the edge** (`cameras-cache.json`); they are NEVER persisted in the cloud DB (control
  stages creds in memory and hands them over once). Don't add cloud persistence for creds.
- **Existing timestamps are RFC3339 `text`** — 58 columns of them, written before sqlx had the
  `chrono` feature enabled. They keep working; read them with `::text` and generate with
  `app_core::timeutil::now_rfc3339()`. **New tables should use `timestamptz`**: the feature is on
  now, so the reason those columns were text is gone. Converting the existing 58 is an
  `ALTER TABLE ... USING col::timestamptz` (RFC3339 parses cleanly, no data lost) but it is a
  migration against a running system, not a flag flip.
- **Migrations**: `crates/app-core/migrations/NNNN_*.sql`, embedded via `sqlx::migrate!`, applied
  on `Db::connect` (control + flow boot). Additive; **ALWAYS `ls crates/app-core/migrations/` to get
  the next number** — no number is written here on purpose, because a stale one here once collided
  with an existing migration and boot-looped control + flow for ~12 minutes. Recent: 0066 =
  `tuya_devices` per-tenant Tuya smart-home catalog, 0060 = node variables, 0061 = faces, 0062 =
  faces labeling, 0063 = faces crop key, 0064 = box hwaccel, 0065 = device stream mode, 0059 = `dictionaries`
  typed-column tables — #113, 0058 = `values_json` on node state, 0050 = generic `work_jobs`
  queue, 0051 = atomic claim + `running` + `run_after`/reaper, 0052 = agnostic `events` stream — card #66 Phase 1,
  0053 = alerts→events migrate + drop, 0054 = per-plan `event_retention_days` + nightly events sweep — #66 Phase 5). **The `work_jobs` queue is the home for async jobs** — clip-delete (#64),
  workflow events, and notification sends run on it. **The `events` table is the product event stream** (alerts,
  presence, clips, auth) — distinct from `platform_logs` (low-level debug trace). **pg_cron is real now** —
  `deploy/postgres/Dockerfile` bundles it; deploy `sentinel-postgres` before control. System
  events (e.g. `motion`, `telegram_message`) are a code catalog (`repo::system_events()`), not a migration.
- **RBAC**: the action catalog is `sentinel-control/src/rbac.rs` `PERMISSIONS`; `seed.rs` upserts it
  into the `permissions` table on boot **and** grants every action to each tenant's system `Admin`
  role. Adding a permission there is enough — without that sync it would reach nobody, since role
  grants are seeded only at tenant creation and the grant screen sits behind `roles.manage`.
  `Viewer` and custom roles are never auto-granted. Any role-scoped query must join `roles` on
  `tenant_id` — the role id comes from the client.
- **Workflow nodes**: the user-visible set is `NodeKind::ALL` (`crates/app-core/src/workflows/domain.rs`) —
  count it there rather than trusting a number here;
  execution in `node.rs` (same folder). Node catalog + port reference: [docs/workflow-nodes.md](docs/workflow-nodes.md).
- **Versioning**: web app = `sentinel-app/package.json` + the footer in `routes/+layout.svelte`;
  edge = `sentinel-edge/Cargo.toml` (drives `LATEST_EDGE_VERSION` + the `v*` release tag). Bump
  together for a release; read the current numbers from those two files rather than from here. The
  edge is **out of the cargo
  workspace** and builds **pure-Rust** — motion is a pure-Rust MOG2 port (#79), detectors are
  `tract`; **OpenCV is an optional, test-only dep** (`opencv-parity` feature), NOT linked in the
  release build. **When you bump `sentinel-edge/Cargo.toml`, sync `sentinel-edge/Cargo.lock` too** —
  `release.yml` builds `--locked` and fails on a stale lock (the `edge-lock-check` CI guard catches it).
- **Ship via GitLab CI** — the repo is `gitlab.com/olharai-grupo/OlharAI`; GitHub is archived
  (read-only). `main` is PROTECTED: no direct pushes, everything arrives by merge request, and
  `check:rust` must pass. Merge to `main` → `.gitlab-ci.yml` builds and deploys the services; tag
  `vX.Y.Z` → the release stage builds the edge Linux binaries and ships them to the VPS
  `/ibbf/edge-dist`. Images live in `registry.gitlab.com/olharai-grupo/olharai/*`. Do NOT use
  `task cloud-deploy`/`edge-publish` (see [deploy ref](docs/ref/deploy.md)).
- **NOTHING IS BUILT LOCALLY. Every build happens in CI/CD.** No `docker build` on a laptop, no
  local `cargo build --release` that ships, no hand-built image pushed to the registry. A build a
  machine can do but CI cannot is a build nobody can reproduce — that is how `sentinel-notification`
  drifted for weeks and took notifications down when a restart finally exercised it, and how the
  repo and production came to disagree about which registry they used.
- **The rule now has NO exceptions** (card #163, closed 2026-08-03). The macOS `.pkg` used to be the
  one hand-built artifact; `release:edge-macos` builds, signs and notarizes it in CI on our own
  Mac (the `macos`-tagged shell runner), and `release:publish` records all three platforms in one
  place. Proven by `v0.5.73`: one tag → Linux x86_64 + aarch64 + a Gatekeeper-accepted `.pkg`, all
  at the same version. **CI runs on SELF-HOSTED runners** (GitLab's shared minutes are exhausted and
  are not being bought) — see [self-hosted runners](docs/plans/self-hosted-runners.md), which also
  records the traps: a job stuck at "Preparing the docker executor" with no container means OrbStack
  is down, or the image is not local yet (#170).
- `/edge-latest` is still answered PER PLATFORM from `/ibbf/edge-dist/manifest.json`, which records
  only what actually got built — a platform whose build failed publishes nothing rather than a
  version that does not exist. The no-platform form returns the MAX version: a "something newer
  exists" trigger, never an install target.
- **GStreamer, linked in-process, is the ONLY media path.** No sidecar, no external decoder, no
  backend flag. `sentinel-edge/src/capture.rs` holds the ONE-PIPELINE-per-camera invariant: a
  consumer camera caps concurrent RTSP sessions at 2–4, so a second connection is how a working
  camera goes dark. Everything else hangs off `tee` branches.
- **The edge is outbound-only** (card #144). No VPN of any kind — the private-mesh era is gone
  from the code as well as the architecture (#199). Cloud→edge calls go over the SSE command
  channel (`app_core::edge_commands::edge_http_via_sse{,_blocking}`) — frames, probes, recordings,
  tracks. A direct HTTP call to a box from a cloud service reaches nothing.
- **No Claude attribution** in commits/PRs (no `Co-Authored-By: Claude`, no "Generated with").
- **sqlx queries are runtime strings** — `cargo check` does NOT validate them. Renumbering or
  removing a bind is invisible to the compiler; a placeholder/bind mismatch took box authentication
  down on 2026-08-01. **CI now catches this**: `check:sql` runs
  `crates/app-core/tests/sql_prepare.rs`, which `PREPARE`s every extracted statement against a
  freshly migrated `sentinel-postgres` and asserts each site's `.bind()` count matches the
  statement's parameter count. It sees only literal SQL — the dynamic-SQL and `QueryBuilder` sites
  it lists as uncovered still need a hand `PREPARE` before shipping.
- UI house style: keep markup logic out of templates (named helpers returning bool/string), no
  nested ternaries; monochrome ink theme (Clean White / Sentinel Noir — no brand hue,
  branding carried by the dot-field form); web-only (no Tauri `prompt`/`confirm`).
- When workflow node kinds/capabilities change, the catalog auto-derives from
  `NodeKind::ALL` — `crates/app-core/src/agent/tools.rs` does NOT list workflow nodes (it's the
  chat agent's `ToolUiEvent`), so no change needed there.
