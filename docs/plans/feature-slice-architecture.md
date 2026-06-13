# Refactor plan — feature-slice modules with domain/adapter separation

## Context & goal

The codebase has drifted into two "god" files that cut across every feature:

- **`app/crates/core/src/db.rs`** — one `Db` struct with the SQL for *every* feature
  (`load_devices`, `load_persons`, `recognize`, `load_alerts`, `load_beacons`,
  `load_workflows`, `load_map`, `load_telegram`, …). It `use`s nearly every domain
  type. Adding a feature means editing this shared file.
- **`app/src-tauri/src/lib.rs`** (~2k lines) — every `#[tauri::command]`, every
  persistence helper, and every background poller for every feature in one place.

We want **feature-slice modules** where each feature owns its full vertical slice, with
a hard separation between **domain (pure logic)** and **adapters (I/O: DB, Telegram,
Anthropic, Tauri)** — i.e. ports & adapters / hexagonal, per slice. This also directly
serves the cloud decomposition (see `feature-slice` ↔ `sentinel-flow`/`sentinel-agent`):
keeping `core` UI-agnostic is what lets the headless services link it without Tauri.

**Non-goal:** no behavior change. This is a pure reorganization — same migrations, same
commands, same runtime behavior. Verified by: unchanged tests, clean build, live boot.

## Target layout (per slice)

A slice spans the two crates because `#[tauri::command]` requires the `tauri` dep and
`core` must stay UI-agnostic. Example — **workflows**:

```
app/crates/core/src/workflows/        (UI-agnostic: domain + adapters that don't need Tauri)
  domain.rs    Graph, NodeKind, Trigger, Edge, validation     — pure (no sqlx/tauri/fs/io)
  engine.rs    run_dag, WorkflowEngine                         — pure decision logic
  node.rs      node execution; reaches the world via ports     — pure + ports
  ports.rs     traits the domain needs (FrameSource, AlertSink, Recognizer, Notifier…)
  repo.rs      Postgres ADAPTER — the workflow SQL (moved out of db.rs)
  mod.rs       re-exports

app/src-tauri/src/features/workflows.rs   (the Tauri ADAPTER)
  the #[tauri::command] handlers + the workflow poller (moved out of lib.rs)
```

### Layering rule (dependency direction — enforced by review)

```
domain.rs ─────────────► (nothing app-specific: std, serde only)
   ▲     ▲
   │     │
repo.rs (sqlx + db-infra)   ports.rs (traits over domain types)
   ▲                            ▲
   │                            │
src-tauri/features/<f>.rs (tauri) ── implements/uses ports, calls repo, manages state
```

- **domain** depends on nothing app-specific. No `sqlx`, no `tauri`, no `std::fs`,
  no `ureq`. Pure types + logic + trait *definitions* (ports).
- **repo** (adapter) depends on `domain` + `db` infra + `sqlx`. Owns all SQL for the slice.
- **external-client adapters** (Telegram `client.rs`, Anthropic `client.rs`, AI sidecar
  `ai_service.rs`) depend on `domain`; they are the outbound ports' implementations.
- **tauri adapter** (`src-tauri/features/<f>.rs`) is the only place with `#[tauri::command]`,
  `AppHandle`, `emit`, pollers. It wires managed state → repo/domain.

## The two decontaminations

### 1. `db.rs` → pure infra + per-feature repos
`db.rs` shrinks to **infra only**: the `PgPool`, the dedicated tokio runtime, `connect`
(+ `migrate!`), and a reusable `block()` / pool accessor exposed `pub(crate)`. It stops
`use`-ing any feature type. Each feature's SQL moves to its `repo.rs`:

| moves out of db.rs → | into |
|---|---|
| `load_devices`/`save_devices` | `devices/repo.rs` |
| `load_persons`/`save_persons`, embeddings, `recognize` | `persons/repo.rs` |
| `load_alerts`/`insert_alert`/`delete_alert`/`update_alert_deliveries` | `alerts/repo.rs` |
| `load_beacons`/`save_beacons` | `notification/repo.rs` |
| `load_workflows`/`save_workflows` (+ engine state, Phase 0.5) | `workflows/repo.rs` |
| `load_map`/`save_map` | `map/repo.rs` |
| `load_telegram`/`save_telegram` | `notification/repo.rs` (telegram config) |

Repos are plain functions taking `&Db` (the infra handle), e.g.
`workflows::repo::load(db) -> Vec<Graph>`. (`migrations/` stays one shared dir — sqlx
runs them centrally; that's infra, not a leak.)

### 2. `lib.rs` → thin assembler + per-feature command modules
Move each feature's `#[tauri::command]`s, its `persist_*` helper, and its poller into
`src-tauri/src/features/<feature>.rs` (commands are `pub fn`). `lib.rs` keeps only:
the Tauri builder, plugin setup, `try_connect_db` hydration orchestration, `.manage(...)`
of state, and `generate_handler![features::workflows::create_graph, …]`.

## How this folds in remaining Phase 0
- **0.5 (engine watermark)** → `workflows/repo.rs`: persist/restore `cron_last` +
  `alert_seen_max` (a `workflow_engine_state` table) when extracting that repo.
- **0.6 (trait seams / ports)** → `ports.rs` in the relevant slices; the in-process
  impls live in the adapters. This is the domain↔adapter boundary itself.

## Execution order (one PR/commit per slice — reversible, bisectable)

0. **Infra first:** carve `db.rs` into `db/mod.rs` (infra: pool, runtime, `block`,
   connect/migrate) — no feature methods yet moved; just expose `pub(crate)` accessors.
1. **`workflows`** — the exemplar (domain/engine/node already mostly pure; add `repo.rs`
   + `src-tauri/features/workflows.rs`; fold in 0.5). Review the shape here.
2. **`alerts`** → `alerts/{domain,repo}.rs` + `features/alerts.rs`.
3. **`notification`** (beacons + telegram + pairing poller) → repo + `features/notifications.rs`.
4. **`persons`** (+ embeddings/recognize, face) → `persons/{domain,repo}.rs` + `features/people.rs`.
5. **`devices`** — the device **registry** only: the `Device` type, ONVIF discovery,
   credential validation, rescan. → `devices/{domain,repo}.rs` + `features/devices.rs`.
   (No streaming/snapshots — those move to `stream`.)
6. **`stream`** (NEW) — everything live-feed + frame related: the **go2rtc** bridge
   (`go2rtc.rs` → `stream/`, a.k.a. `sentinel-rtc`), **snapshots** (`device_actions::capture_snapshot_bytes`
   + the ONVIF/ffmpeg fallback), the **live-feed** open (`show_live_feed`/`ensure_streams`),
   and the **keep-warm** poller. → `core/src/stream/` + `features/stream.rs`. This slice has
   **no Postgres repo** — its "adapter" is the go2rtc child process + ONVIF/RTSP, so its
   ports are `FrameSource`-shaped, not SQL. It maps to the **edge tier** (`sentinel-edge`
   wrapping `sentinel-rtc`) and is the foundation the future **`sentinel-record`** module
   builds on. `stream` depends on `devices` (reads a `Device` to find its RTSP URL).
7. **`map`** → `map/{domain,repo}.rs` + `features/map.rs`.
8. **`agent`** (chat) — already close; formalize ports (tools reach world via `ToolContext`
   which becomes the port bundle) + `features/chat.rs`.
9. **small slices**: `settings`, `voice`, `listen` → `features/*.rs` adapters.

After each slice: `db.rs` loses that feature's methods; `lib.rs` loses that feature's commands.

## Verification (every slice, no behavior change)
- `cargo check -p app` + `cargo test -p app-core` green.
- `grep` confirms `db.rs` no longer references the moved feature's types; `lib.rs` no
  longer defines the moved commands.
- Live boot: app starts, the feature's commands work from the UI, migrations unchanged.
- Domain purity check: `grep -nE 'sqlx|tauri|std::fs|ureq' core/src/<feature>/domain.rs`
  returns nothing.

## Risks / notes
- **Big mechanical diff per slice** — mitigated by one-slice-at-a-time + tests + the
  "no behavior change" rule.
- **`#[tauri::command]` must stay in `src-tauri`** — accepted (and desired: it keeps
  `core` linkable by the future headless services).
- **Shared state types** (`DeviceStore`, `AlertStore`, …, `Arc<Mutex<Vec<_>>>`) remain
  core domain types; the Tauri adapter owns `manage()`/hydration. No change there.
- **Ports vs over-abstraction** — only introduce a trait where a real second impl is
  coming (the cloud REST adapters). Don't trait-ify pure functions for their own sake.
