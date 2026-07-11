# Agnostic Event Module — replace Alerts with a unified event stream — Implementation Plan

**Board card:** Agnostic event module — replace the Alert model (ToDo)
**Goal:** Retire the legacy `alerts` model and replace it with one **agnostic event stream** that
every subsystem registers into — camera clip start/stop, workflow custom + system events, user
login, presence — with per-tenant retention tied to the plan, robust enough to later drive a
time-travel Live view (events pinned to the right moment of video playback).

## Why now / the north star

A future **Live module** will show online + past clips on a timeline with events overlaid at
their exact playback moment ("time travel"). That is only possible if every meaningful thing
that happens becomes a first-class, precisely-timestamped, entity-linked **event**. Today that
data is scattered across three half-overlapping systems. This plan unifies them. (The Live
timeline UI itself is a **separate plan** — here we only build the event substrate it needs.)

## Current state — three overlapping systems (grounding)

1. **`alerts`** — migration `0004_alerts.sql`, module `crates/app-core/src/alerts/{mod,repo}.rs`.
   In-memory `AlertStore = Arc<Mutex<Vec<Alert>>>` + per-tenant Postgres rows (`id` bigint,
   `level`, `message`, `room_id/name`, `source`, `created_at`, `deliveries` jsonb).
   - **Producers:** workflow Alert node (`workflows/node/alerts.rs:57`), chat agent tools
     (`agent/tools.rs:599`), control manual create (`sentinel-control/src/domain/alerts.rs:84`).
   - **Consumers:** frontend `sentinel-app/src/routes/alerts/`, beacon delivery
     (`alerts::deliver` → `notification::dispatch`). **This is what we replace.**
2. **`tenant_events` + `tenant_event_categories`** — migrations `0015`, `0022`. A per-tenant
   **registry of event *types*** (name, category, `source` `user|system`, `payload` schema),
   powering workflow `EmitEvent`/`OnEvent` cross-graph fan-out. `sentinel-flow/src/engine.rs:499`
   `emit_event(...)` is an existing seam explicitly reserved for "login, user-create, chat,
   alerts…"; `domain.rs:1599` echoes the same intent. **Keep as the type catalog** — occurrences
   are new.
3. **`platform_logs`** — migration `0028`, `crates/app-core/src/platform_logs.rs`. Already
   agnostic (`module`, `trace_id`, `entity_id/type`, `level`, `message`, `payload`, `created_at`)
   with an async `LogQueue` + sync insert and a **flat 30-day** pg_cron TTL. It's a debug/audit
   trace, not a user-facing, plan-retained timeline. **Keep for low-level tracing;** don't
   overload it as the product event stream (see open decision 1).

Retention model to mirror: `billing_plans.retention_days` (`0047`: beta 3 / home 30 / business
60) enforced by pg_cron + R2 key-prefix lifecycle for recordings (`clip_store.rs`).

## Phase 1 — The agnostic event core (ships a unified feed) — **shipped**

Decisions locked: dedicated `events` table (not `platform_logs`); full replacement of `alerts`
(migrate + delete) in Phase 2; `bigserial` PK + `(tenant, occurred_at)` ordering. Built as
migration **0052** (not 0050 — occupied by `work_jobs`). `crates/app-core/src/events.rs` mirrors
`platform_logs` (NewEvent + EventQueue async + sync insert + `list_events`). Control feed API
`GET /events` (cursor + filters) + `GET /events/kinds` in `domain/events.rs` (guarded `alerts.read`).
Frontend `/events` feed page (dot-field severity, filters, auto-refresh, cursor "load more"),
added to the tenant nav; the dead `/beacons/notifications` nav item removed. First producer wired:
`auth.login` on sign-in, so the feed is live end-to-end. **SSE deferred** — the feed uses
auto-refresh polling for now (same pattern as the load panel); an SSE live feed can front it later.

- Migration **0052** `events`:
  `id bigserial PK`, `tenant_id uuid`, `kind text` (stable catalog slug, e.g.
  `alert`, `presence`, `clip.started`, `clip.stopped`, `workflow.custom`, `auth.login`),
  `source text` (`system` | `user` | `workflow` | `edge`), `severity text`
  (`debug|info|warn|error`), `title text`, `message text`,
  entity refs for timeline linkage: `camera text`, `clip_id text`, `room_id bigint`,
  `room_name text`, `device_id text`, `graph_id bigint`, `user_id uuid`,
  `data jsonb` (typed payload), `occurred_at text` RFC3339 (`now_rfc3339()`), `created_at text`.
  Indexes: `(tenant_id, id DESC)`, `(tenant_id, kind, id DESC)`,
  `(tenant_id, camera, occurred_at DESC)` (the time-travel query),
  `(tenant_id, occurred_at DESC)`.
- New module `crates/app-core/src/events/{mod,repo}.rs` — domain `Event` + `EventKind` catalog
  (a code catalog like `repo::system_events()`, not a migration), plus `repo::{insert, list,
  query}` and an async `EventQueue` mirroring `platform_logs::LogQueue` (fire-and-forget mpsc →
  batched insert), and a sync path for the flow engine threads.
- Control read API: `GET /events` (paginated, filter by kind/severity/camera/time-range) in a
  new `sentinel-control/src/domain/events.rs`, plus an SSE live feed (reuse the presence/runs SSE
  pattern). **Value on its own:** a single Events feed page.

## Phase 2 — Replace Alerts with events — **shipped**

Full replacement (migration 0053 migrates rows → events, drops the `alerts` table; the
`alerts` module is deleted). `now_rfc3339`/`is_today` relocated to `app_core::timeutil` first.
Producers all emit `alert` events: the CreateAlert/DeliverAlert/GetLastAlert workflow nodes
(rewired, kept working), the chat agent's alert tools, and control's `/alerts` endpoints (now a
thin events view — the frontend page is unchanged). Beacon delivery centralised in
`events::deliver_event_to_beacons`. Dead OnNewAlert engine machinery removed.

- Rewrite the three alert producers to emit `events` (kind `alert`, carrying `severity` + room):
  `workflows/node/alerts.rs`, `agent/tools.rs`, `sentinel-control/src/domain/alerts.rs`.
- Repoint **beacon delivery**: `alerts::deliver` logic moves to operate on an event
  (`notification::dispatch` unchanged); deliveries recorded in the event's `data` (or a
  `event_deliveries` child — open decision).
- **Migrate** existing `alerts` rows → `events` (kind `alert`) in the migration; stop writing the
  `alerts` table and delete the `AlertStore` in-memory path + `crates/app-core/src/alerts/`
  module (keep `now_rfc3339`/`is_today` — relocate to a shared util; many modules import them).
- Frontend: `routes/alerts/` becomes a filtered view of the new events feed (kind = alert) or is
  folded into the Events page.

## Phase 3 — Every subsystem registers events

- **Presence** (`0020` + `RecordPresence`): each presence record also emits a `presence` event
  (camera, room, category, label) so the map pulse and the timeline share one source. Decide
  whether presence rows are *projected* as events or *dual-written* (open decision 3).
- **Recording lifecycle:** the edge already confirms clip upload to control
  (`sentinel-control/src/http.rs:560`); the edge ring buffer knows start/stop. Emit
  `clip.started` / `clip.stopped` events keyed by `clip_id` + `camera` + `started_at`/duration —
  these are the anchors the time-travel view scrubs to.
- **Auth:** `login` (`handlers.rs:133`) and `assume_tenant` (`:324`) emit `auth.login` events via
  a control-side event writer (or the `sentinel-flow` `emit_event` seam).
- Route **system events** through the existing `emit_event` seam so they *also* fan out to
  workflow `OnEvent` listeners where a matching type exists.

## Phase 4 — Workflow pure emit node

- A user-facing node to **push a custom event into the stream** with a chosen kind/category +
  typed payload, reusing `EmitEvent`'s existing payload-port machinery
  (`domain.rs:1433 payload_ports`). Decide: extend `EmitEvent` (today it only fans out to
  `OnEvent`, no persistence) to also record an occurrence, vs. a new dedicated node
  (open decision 4). Update `NodeKind::ALL` + `docs/workflow-nodes.md`; the agent-tools catalog
  auto-derives (no change needed per project note), but keep it verified.

## Phase 5 — Per-tenant retention + timeline readiness

- pg_cron nightly sweep deleting events older than **the tenant's plan `retention_days`** — one
  job that joins `events` → newest `tenant_subscriptions` → `billing_plans.retention_days`
  (mirrors the recordings retention model; do **not** use platform_logs' flat 30 days).
- Confirm the schema carries precise `occurred_at` + `camera`/`clip_id`/duration so the future
  Live time-travel view can place each event at the right playback offset. Verify the
  `(tenant_id, camera, occurred_at)` index serves that query.

## Open decisions (grill before building)

1. **New `events` table vs promote `platform_logs`?** Recommend a **dedicated `events`** table
   (user-facing, plan-retained, timeline-indexed) and leave `platform_logs` as the low-level
   debug/audit trace. Merging risks 30-day debug spam drowning the product timeline. Agree?
2. **Legacy `alerts` fate:** full replacement — migrate rows into `events`, delete the module +
   table (recommended) — or keep `alerts` as a read-only view over `events`?
3. **Presence & recordings:** dual-write events, or make `events` the single store and *project*
   presence/clip rows from it? (Recordings must stay their own table for billing/storage; the
   question is only about the event mirror.)
4. **Custom-event node:** extend `EmitEvent` to persist occurrences, or add a separate "Record
   Event" node so fan-out and occurrence-logging stay distinct concepts?
5. **Event id & ordering:** `bigserial` global PK + `(tenant_id, occurred_at)` index
   (recommended for timeline) vs per-tenant monotonic bigint (as alerts/presence do today).

## Out of scope (v1)

- The **Live time-travel timeline UI** — its own plan; this plan only builds the substrate.
- Per-event-kind notification routing rules / subscriptions.
- Event correlation graph, exports, and analytics/aggregation.
- Changing recordings storage or billing (retention_days is reused as-is).
