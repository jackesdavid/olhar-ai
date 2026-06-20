# Workflow Runs — Real-Time (drop the 700 ms poll) — Implementation Plan

**Use case:** _(none — performance ask: "the workflow UI polls the backend, unperformant; can we do better with WebSocket/SSE?")_
**Board card:** Workflow runs: kill the 700 ms poll, go real-time over SSE (ToDo)
**Goal:** Replace the redundant 700 ms `runStates()` poll with the SSE stream that already exists, so live run badges + current-node highlight update in real time at ~zero idle cost.

## Why (root cause)

The workflows page polls `GET /workflows/runs` every **700 ms** (`sentinel-app/src/routes/workflows/+page.svelte:798-804`) to get the live `activity` map (`running` count per graph + `current` node id). Meanwhile the same page is **already** connected to an SSE stream (`data.ts:44-90` → control relay `sentinel-control/src/domain/workflows.rs:274-319` → flow broadcast bus `sentinel-flow/src/events.rs:22`) receiving `workflow-run-start`, `workflow-run`, `workflow-node`, `workflow-log`. The poll is pure redundancy: the only state it carries that SSE doesn't is the in-memory `GraphActivity` map (`sentinel-flow/src/state.rs:33-43`), which simply was never published to the bus.

**SSE, not WebSocket.** Updates are server→client only; the broadcast bus + tenant-scoped relay + frontend `EventSource` demux already exist. WebSocket adds a bidirectional channel we don't need.

## Phase 1 — Publish activity deltas on the existing bus (backend, flow)
- In `sentinel-flow/src/engine.rs`, at the points that mutate the activity map (`mark_run_started` / per-node `current` update / `mark_run_finished`, around lines 130-152 / 176-187), publish a new `workflow-activity` event via `events::publish()` carrying the **single graph's** new `GraphActivity` (`{ graph_id, running, current, run_id, trigger, pod_name }`). Reuse the existing `tenant_id`-tagged publish path so the control relay's tenant filter (`workflows.rs:309-312`) works unchanged.
- Keep the payload a per-graph delta, not the whole map — matches how `workflow-node` already emits per-event and keeps the broadcast small.
- No new channel, no new migration: `GraphActivity` already derives `Serialize` (`state.rs:33`).

## Phase 2 — Consume it in the frontend, delete the poll
- In `+page.svelte`, add an `onEvent("workflow-activity", …)` listener (next to the existing `workflow-run` / `workflow-node` handlers ~lines 727-780) that merges the delta into the local `activity` map and recomputes badges/highlight.
- **Delete** the `setInterval(…, 700)` block (lines 798-804) and its `clearInterval` (line 815). Keep `data.runStates()` as a **one-shot fetch on mount** to seed initial state (covers runs already in flight before the page opened), then rely on SSE for updates.
- Verify the current-node highlight + running badge still update by watching a manual run.

## Phase 3 — Hardening (reconnect + seed-on-reconnect)
- The shared `EventSource` (`data.ts:55`) auto-reconnects, but on reconnect we can miss deltas fired while disconnected. On the SSE `onopen`/reconnect, re-run the one-shot `runStates()` seed so the map re-syncs. (Cheap: one request per reconnect vs. 85/min.)
- Confirm `run_history` drawer + `run_logs` stay on-demand (already correct — no change).

## Open decisions (grill before building)
1. **Per-graph delta vs. whole-map snapshot on `workflow-activity`?** Delta is lighter and matches existing event granularity, but the client must already hold the map (it does, seeded on mount). Recommend delta.
2. **Coalescing:** node-heavy runs could emit a `workflow-activity` per node. Do we throttle/coalesce activity events (e.g. skip if only `current` changed and a `workflow-node` already carries that), or just fold `running`/`current` into the existing `workflow-node` payload and add **no** new event type? Folding into `workflow-node` is the smallest change but couples two concerns.
3. **Seed-on-reconnect** (Phase 3): ship in v1 or defer? Without it, a dropped connection can leave a stale badge until the next run. Recommend ship it — it's a few lines.

## Out of scope (v1)
- WebSocket transport (not needed; SSE suffices for server→client).
- Reworking `run_history` / `run_logs` (already on-demand, not polled).
- Any DB/migration change (activity is in-memory only).
- Multi-tab dedup of the SSE connection (already a single shared `EventSource`).
