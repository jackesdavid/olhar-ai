# sentinel-flow

The always-on **workflow engine service**. Phase 1 of the cloud decomposition
(see `.claude/plans/yes-we-need-to-pure-fog.md`): the workflow engine, the trigger
poller, and the run executor move **out of the Tauri process** into this standalone
service so the UI/control-plane can be stopped and restarted freely while
automations keep firing 24/7.

It links the same UI-agnostic `app-core` the Tauri shell uses — the engine, the
typed nodes, the Postgres repos, and the Phase 0.6 boundary ports — and talks to the
same shared Postgres. The UI becomes a **client**: it lists graphs, asks for runs,
and subscribes to a live event stream.

## Architecture

```
            ┌──────────────── sentinel-flow (this) ────────────────┐
            │  std threads (blocking)            tokio (async)      │
   Postgres │  ┌───────────────┐   Arc<AppState> ┌──────────────┐  │
 ◀────────▶ │  │ trigger poller │◀──────────────▶│ axum: REST   │  │ ◀── UI / Tauri
 (shared)   │  │ run executor   │   SSE EventBus │   + SSE       │  │     (client)
            │  └───────────────┘ ───────────────▶└──────────────┘  │
            └──────────────────────────────────────────────────────┘
```

- The **engine + poller + executor** run on dedicated std threads (blocking),
  mirroring the Tauri shell. `app-core`'s `Db` owns its own runtime and bridges
  sqlx via a channel (never `block_on` the current thread), so it's safe to call
  from there and from axum handlers alike.
- **Boot is synchronous** (before any tokio runtime exists) because `Db::connect`
  drives migrations with `block_on` — running it inside the server runtime would
  panic ("runtime within a runtime").
- **Engine state is restored from Postgres** (Phase 0.5): the `OnNewAlert`
  watermark and per-graph cron timers, so a restart neither replays the alert
  backlog nor fires a long cron immediately.
- **SSE is an optimization, never the source of truth** — final state is always
  re-fetchable via REST. Raw image bytes never go on SSE (it's one UTF-8 stream);
  image events carry only a `{graph,node,label}` reference.

## Run

```sh
DATABASE_URL=postgres://sentinel:sentinel@localhost/sentinel \
FLOW_BIND=127.0.0.1:8787 \
cargo run -p sentinel-flow          # or: task flow
```

Start Postgres first (`task db-up`). Optional env:

| Var | Default | Purpose |
|---|---|---|
| `DATABASE_URL` | `postgres://sentinel:sentinel@localhost/sentinel` | shared DB |
| `FLOW_BIND` | `0.0.0.0:8787` | listen address |
| `SENTINEL_MODELS_DIR` | — | YOLOX / ArcFace models (person/face nodes) |
| `TELEGRAM_BOT_TOKEN` / `TELEGRAM_PAIRING_CODE` | — | notification fallback when not set in Postgres |
| `RUST_LOG` | `info` | log level |

## API

| Method | Path | Purpose |
|---|---|---|
| `GET` | `/health` | liveness + loaded-graph count |
| `GET` | `/graphs` | the named graphs (triggers, nodes, edges) |
| `POST` | `/graphs/reload` | re-read graphs from Postgres (UI edits land there) |
| `GET` | `/runs` | live run state per graph (`{running, current}`) |
| `POST` | `/runs` | `{ "graph_id": N }` → `202` (async job; progress on `/events`) |
| `GET` | `/events` | SSE stream: `workflow-run(-start)`, `workflow-node`, `workflow-log`, `workflow-print`, `workflow-image`, `snapshot`, `alerts-changed`, `devices-changed`, `open-live` |

### Milestone check (the constraint this proves)

Start a workflow with a short cron, then stop and relaunch the Tauri app — the
crons keep firing here, independent of the UI. Restart `sentinel-flow` itself
mid-interval and it does **not** fire immediately on boot (persisted engine state).

## Not yet (later phases)

Out-of-band binary frame fetch (`GET /runs/{id}/frames/{n}`), idempotency keys on
mutating calls, `Last-Event-ID` replay from a ring buffer, single-leader election,
and a durable run queue. See Phases 2–5 in the decomposition plan.
