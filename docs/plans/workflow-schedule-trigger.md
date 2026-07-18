# Scheduled ("cron") workflow trigger — calendar schedule builder — Implementation Plan

**Board card:** Scheduled workflow trigger — calendar schedule builder (ToDo)
**Goal:** Start a workflow graph on a recurring **calendar** schedule chosen from dropdowns —
every hour, N times a day, daily at a time, weekly on chosen weekdays, monthly on a day, yearly —
durably (survives flow-pod restart), no raw cron syntax.

## What already exists (reuse, don't reinvent)
A trigger is a **graph-level property, not a node** — `domain.rs:1594-1610`
`enum Trigger { Manual, Cron { every_secs } }` (doc: *"A property of the engine/graph — NOT a
node"*), stored on `Graph.trigger` (`domain.rs:1765`, default `Manual`).
- **Poller:** `sentinel-flow/src/engine.rs:41-75` (`spawn_poller`, 1s tick) → `engine.cron_due()`
  (`crates/app-core/src/workflows/engine.rs:1160-1177`) → `fire_graph(…, TriggerSource::Cron, …)`
  → `spawn_graph_run` (full-DAG run, `engine.rs:77-175`).
- **Durable cron state:** table `workflow_cron_state(graph_id, last_fired_ms)`
  (`migrations/0009_workflow_engine_state.sql:19-22`), saved on fire (`repo.rs:180`), restored at
  boot into an `Instant` anchor (`state.rs:194-208`, `seed_cron_last` `engine.rs:1150`).
- **Durable queue:** `work_jobs` (UNLOGGED, migrations 0050/0051) with `run_after` (RFC3339 text) —
  "claimable once run_after <= now" — plus atomic claim + reaper. Future-enqueue primitive
  `work_queue::enqueue_at` (`crates/app-core/src/work_queue.rs:82,223`), already used for
  DelayTrigger window stops (`work_queue.rs:294-309`). Consumer template:
  `sentinel-flow/src/window_consumer.rs:28-45` (poll `claim_blocking`, act).
- **pg_cron** is installed (`deploy/postgres/Dockerfile`, `migrations/0046_pg_cron_jobs.sql`) for
  pure-SQL periodic sweeps.
- **Trigger config API:** `PUT /workflows/:id/trigger` → `set_trigger`
  (`sentinel-control/src/domain/workflows.rs:58,864-888`), body `TriggerReq { kind, every_secs }`;
  **force-disables the graph on any trigger change** (881-883). Edits propagate to the flow pod via
  `poke_reload()` → `POST {FLOW}/graphs/reload` (`workflows.rs:94-119`, `state.rs:136`).
- **Editor UI:** trigger `Select` at `+page.svelte:3616-3640`, `TRIGGER_OPTS` (L2661),
  `triggerKind/Secs/Summary` (L1590-1599), `applyTrigger` (L1634) → `data.setGraphTrigger`
  (`lib/data.ts:309`). TS `Trigger` type at `+page.svelte:107`.

**Why the existing `Cron { every_secs }` isn't enough:** it's a stopwatch (interval since last
fire), with **no time-of-day, no weekday/day-of-month, and no timezone anywhere in the model**.
"Daily at 9am" is inexpressible as one `every_secs`.

## Chosen architecture (locked with the user): durable, DB-backed
Compute the **next occurrence in Rust** (chrono + chrono-tz) and drive firing off the **DB**, not
an in-memory calendar clock — so it survives a flow-pod restart and one clock serves all tenants.
Two concrete sub-designs; **the user leaned toward the `work_jobs` queue (Option B)** — lead with
it, but Phase 0 confirms against the lower-risk Option A since both are DB-durable:

- **Option B (queue — user's steer):** on schedule set/fire, compute `next_fire_at` and
  `enqueue_at` a `work_jobs` row (`category='workflow_schedule'`, `run_after=next_fire_at`). A new
  flow consumer copies `window_consumer.rs` to `claim_blocking` due rows → `fire_graph(…,
  TriggerSource::Cron, …)` → then re-enqueue the following occurrence. Fully DB-durable, decoupled
  from the poller.
- **Option A (extend the existing poller — lowest-risk):** add a `Trigger::Schedule` arm in
  `cron_due` (`engine.rs:59-68`) that compares now vs a persisted `next_fire_at`; reuse the
  already-wired `workflow_cron_state` durability (persist-on-fire, restore-on-boot). Same
  `fire_graph` path.

Both reuse `fire_graph`/`TriggerSource::Cron` — **do NOT add a new `TriggerSource` variant**
(avoids the `0056` CHECK-constraint migration trap that bit #86, where a missing migration
silently drops the run's own diagnostics). A scheduled run is just a full-DAG fire.

## Phase 0 — Lock the timer design (Option B vs A)
- Decide queue (B) vs poller (A). Recommendation: **B** per the user, since re-enqueueing the next
  occurrence is a clean fit for calendar recurrence and keeps calendar math out of the hot poller.
- Confirm migration number on disk (CLAUDE.md says next is 0055; the trigger-check lives at 0056 —
  **verify the real highest** before numbering).

## Phase 1 — Domain: a structured `Trigger::Schedule`
- `domain.rs:1594-1610`: add `Trigger::Schedule { freq, time_of_day, weekdays, day_of_month,
  month, tz }` (keep `Manual`/`Cron`). Model `freq` as an enum: `Hourly | Daily | Weekly | Monthly
  | Yearly` (+ "N times a day" = a set of times-of-day, or an every-N-hours variant). Serde-tagged
  so it round-trips through the graph JSON.
- Add **chrono + chrono-tz as normal workspace deps** (NOT sqlx features — sqlx stays without
  chrono/time; timestamps remain RFC3339 text via `app_core::timeutil::now_rfc3339()`). Use them
  only for "next occurrence from now in tz" math.
- `next_occurrence(schedule, from, tz) -> DateTime` helper in app-core (unit-testable, pure).

## Phase 2 — Durable firing (per Phase 0 choice)
- **B:** new `migrations/00NN_workflow_schedule.sql` — persist the schedule (already on the graph
  JSON, so possibly just a `work_jobs` category + an index). Enqueue-next on set and after each
  fire; new `sentinel-flow/src/schedule_consumer.rs` modeled on `window_consumer.rs` →
  `fire_graph`. Re-enqueue the following occurrence inside the consumer.
- **A:** extend `workflow_cron_state` with `next_fire_at`/`schedule_json`; add the `Trigger::Schedule`
  arm to `cron_due`; persist/restore alongside the existing cron path.
- **Missed-fire policy:** decide backfill vs skip when the pod was down across a due time (compare
  persisted `next_fire_at` to now). Default: fire once on recovery, then advance — no storm of
  backfills.

## Phase 3 — Control API
- Extend `TriggerReq` + `set_trigger` (`workflows.rs:858-888`) to accept the structured schedule
  (`kind:"schedule"` + the fields). Keep the existing force-disable-on-change or relax it for
  schedule edits (UX call — see decisions). `poke_reload` already propagates to flow.

## Phase 4 — Editor UI: the dropdown builder
- `+page.svelte`: add "Schedule" to `TRIGGER_OPTS` (L2661) and the trigger `Select` (L3616-3640).
  Reveal sub-dropdowns based on `freq`: time-of-day picker (Daily/Weekly/Monthly/Yearly), weekday
  multiselect (Weekly), day-of-month (Monthly), month (Yearly), and a timezone select.
- Extend the TS `Trigger` type (L107), `triggerSummary` (a human line: "Every Mon & Wed at 09:00"),
  `applyTrigger` (L1634), `data.setGraphTrigger` (`lib/data.ts:309`). Keep markup logic in named
  helpers (house style — no nested ternaries).
- Timezone default: per-graph, defaulting to the tenant's/browser tz.

## Open decisions (grill before building)
1. **Timer mechanism: `work_jobs` queue (B) vs existing poller + `workflow_cron_state` (A).** User
   leaned B. B matches "queue + cron on db"; A reuses more wired-up machinery. **Biggest call.**
2. **Timezone ownership.** No tz exists in the model today — this is the single biggest new
   concern. Per-graph tz (flexible) vs per-tenant tz (simpler)? "Daily at 9am" is meaningless
   without one. Recommend per-graph with a tenant default.
3. **"N times a day" shape.** A list of explicit times-of-day, or an "every N hours" interval? A
   list is more predictable; interval is simpler UI. Recommend explicit times list.
4. **Force-disable on edit.** `set_trigger` disables the graph on any change today. Keep for
   schedule edits, or preserve enabled state? UX call.
5. **Granularity.** Minute is plenty (poller ticks 1s / queue is run_after-precise). Confirm no
   sub-minute need.

## Out of scope (v1)
- Raw cron-expression entry (dropdowns only).
- Per-run schedule overrides / one-off "run at" (this is recurring); a one-shot could reuse
  `enqueue_at` later.
- Calendar/holiday awareness, "last weekday of month", nth-weekday — advanced recurrence beyond the
  dropdowns above.
- Backfilling many missed fires after long downtime (fire-once-on-recovery only).
