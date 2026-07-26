# Structured logging — tenant + edge context on every line, searchable fields

**Board card:** #146 structured logs (tracing → Datadog; tenant_id + edge_id/name; searchable fields)
**Goal:** Every log is **structured JSON** with each datum its own searchable field, and carries
**tenant_id** and — for edge logs — **edge_id + edge_name**, injected in ONE central place, not
per call site. So `capture[192.168.10.101]: mode=preview 640x480 grab=0.0ms decode=8.5ms 40 frames
(4.0 fps)` becomes `{camera_ip, mode, width, height, grab_ms, decode_ms, frames, fps, tenant, edge_id,
edge_name, …}` in Datadog, filterable on any of them.

## Why this needs `tracing`, not `log`
Today every crate uses the **`log`** crate + `env_logger` (control/flow) and a custom `RingLogger`
(`sentinel-edge/src/logbuf.rs`). `log` records are **plaintext only** — `log::info!("...{}...")`
flattens everything into one `message` string (which is why the edge relay ships a text blob).
Structured fields require **`tracing`**, whose events carry typed key/value fields and whose
**spans** carry ambient context (tenant, edge) that every event inside inherits — the "central root
place." `tracing-log` bridges existing `log::*` calls so nothing breaks during the migration; sites
get converted to structured fields incrementally.

## Central context — the "root place"
- **Control / flow (per request):** an Axum middleware opens a span `tenant=<id>` from the
  `UserClaims`/box token (mirrors `tenant_of`); every event during that request inherits it. No log
  site passes tenant_id.
- **Edge (single-tenant process):** a **root span** opened at boot carries `edge_id` (the box_id
  from `BOX_TOKEN`, already known) + `edge_name`. Since the whole process is one tenant/box, every
  edge log inherits it. `edge_name` isn't known on the box (it holds the token, not the name) →
  control resolves it from `tenant_boxes.name` when the relay lands (cached), OR the config poll
  returns it and the edge stamps it. (Open decision.)

## Phases

### Phase 1 — adopt `tracing`
- Add `tracing` + `tracing-subscriber` (+ `tracing-log` for the compat bridge) to control, flow,
  edge. Replace `env_logger::init()` / the edge `RingLogger` install with a `tracing-subscriber`
  registry. Existing `log::*` calls keep working (bridged) — plaintext for now, converted later.

### Phase 2 — the JSON + Datadog layer
- A custom `tracing_subscriber::Layer` that turns each event into a JSON object: **every field a
  top-level key**, plus `level`, `target`, `timestamp`, and the ambient span fields (`tenant`,
  `edge_id`, `edge_name`). Two sinks: pretty to stderr (dev), and — replacing `app_core::datadog`'s
  current per-record shaping — batch-ship the JSON to Datadog. This *supersedes* the #145
  `platform_logs`→Datadog record builder: the log record IS the structured event now.

### Phase 3 — central context spans
- Control/flow: the tenant middleware (`from_fn_with_state`) opens the `tenant` span.
- Edge: `run()` opens the root `edge_id`/`edge_name` span before spawning workers.

### Phase 4 — edge relay carries structure + identity
- The edge already relays lines to `/config/logs`. Change `LogLineIn` (control `store.rs`) + the
  edge push to carry **structured fields** (a `fields: serde_json::Value`) and the `edge_id`, not
  just `{ts, level, target, message}`. Control stamps `tenant` (from the box token) + `edge_name`
  (box-name lookup) and ships the whole structured event to Datadog. `box_logs_push` already routes
  to the #145 sink — it just forwards richer records.

### Phase 5 — convert high-value log sites to fields
- Start with the ones that carry real telemetry, `camera_ip` first (the example):
  `frames.rs` capture, `recording.rs`, `motion.rs`, `command_stream.rs`, the workflow engine.
  `tracing::info!(camera_ip=%ip, mode, width=w, height=h, grab_ms=grab, decode_ms=decode, frames, fps, "capture")`.
  The rest stay bridged (plaintext `message`) and convert opportunistically — no big-bang rewrite.

## Open decisions
1. **edge_name source** — control resolves it from `tenant_boxes.name` at relay time (one lookup,
   cached), OR the config poll returns the name and the edge stamps its own span. (Lean: control
   resolves — keeps the box dumb, name changes take effect immediately.)
2. **Datadog reserved attributes** — map to DD's conventions (`host`, `service`, `status`,
   `ddtags`) so facets + the log pipeline work out of the box; `camera_ip` etc. become custom
   attributes (searchable via `@camera_ip:`).
3. **Volume** — structured events are bigger than lines; confirm the Datadog free-tier ingestion cap
   with level-filtering (ship `info`+ from the edge; `debug` only when a box is being debugged).

## Out of scope (v1)
- Converting **every** `log::*` call site to structured fields at once — the `tracing-log` bridge
  keeps them working; convert the telemetry-carrying ones first, the rest over time.
- APM / distributed traces / metrics — logs only (metrics already ride the heartbeat).
- Changing what data lives where (that's #145) — this is purely the shape + context of the logs.
