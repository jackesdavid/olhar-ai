# Event-storm backstop never engages (WF_GLOBAL_CAP dead)

**Board card:** #91 Event-storm backstop never engages (WF_GLOBAL_CAP dead) (Bugs)
**Found while:** planning [[editor-exec-fan-in]] (#90), answering "if max_concurrent = 10 and 10 Telegram
messages arrive, do all 10 graphs run concurrently?" — they do, because this ceiling can't stop them.

## The bug

The event fan-out has a pod-wide safety net (`sentinel-flow/src/engine.rs:449`):
```rust
// Global backstop: don't let an event storm exceed the pod's run ceiling.
if state.engine.lock().unwrap().total_running() >= WF_GLOBAL_CAP {
    // publish skip { reason: "capacity" }; continue;
}
```
**It can never fire for events.** `total_running()` (`crates/app-core/src/workflows/engine.rs:961`) sums
`WfEngine.runs`:
```rust
pub fn total_running(&self) -> u32 { self.runs.values().map(|r| r.running).sum() }
```
`WfEngine.runs.running` is incremented **only** by `on_spawn`, which is called from exactly two places —
`sentinel-flow/src/engine.rs:83` (inside `fire_graph`) and `:160` (draining a `QueueSingle` fire). Both are
the **cron/manual** path.

The event path never touches it. `spawn_event_handler` calls `mark_run_started` (`engine.rs:191`), which
increments `state.activity` — a **different** map, feeding the UI's `workflow-activity` SSE:
```rust
let mut a = state.activity.lock().unwrap();
let e = a.entry((tenant, graph_id)).or_default();
e.running += 1;
```
So during a pure event storm `total_running()` is ~0 and the ceiling never engages. `WF_GLOBAL_CAP = 8`
(`sentinel-flow/src/state.rs:50`) bounds cron fires only — not the event storms it was written for.

## Impact

A graph on `Concurrent { max_concurrent: N }` triggered by N events (e.g. N Telegram messages) spawns N
real OS threads with **no pod-level ceiling**. The only bound is the per-`(tenant, graph, event)`
`state.event_runs` counter (`engine.rs:~470`), which is exactly `max_concurrent`:
```rust
.map(|(_, g)| match &g.concurrency {
    ConcurrencyPolicy::Concurrent { max_concurrent } => *max_concurrent as usize,
    _ => 1,
})
```
Set `max_concurrent: 10` and you get 10 concurrent runs, not the 8 the backstop intends. With slow AI/HTTP
handlers that's 10 threads regardless of pod health. (At capacity the next event is **dropped**, not
queued — `Concurrent` has no queue.)

## Fix options (decide at implementation)

- **(a) Recommended — make event runs reserve a slot.** Have `spawn_event_handler` call `engine.on_spawn`
  before spawning and `on_finish` at the end, so `total_running()` counts them. Care: `on_finish` returns a
  "drain queued" bool used by `QueueSingle` (`engine.rs:158-161`) — the event path must **not** drain that
  queue. One source of truth, matches the backstop's intent.
- **(b) Smaller — read the map that's actually incremented.** Have the backstop sum `state.activity`
  instead of `WfEngine.runs`. Less invasive, but leaves two counters meaning "running".

Either way there are now **three** overlapping counters — `WfEngine.runs`, `state.activity`, and
`state.event_runs` — each incremented by a different path. Consider consolidating, or at minimum document
who owns what.

## Verification

- Set a graph to `Concurrent { max_concurrent: 20 }`, fire 20 events at once → assert at most
  `WF_GLOBAL_CAP` run simultaneously and the remainder log the skip with `"reason": "capacity"`.
- Confirm cron fires still respect the cap (`engine.rs:46`) and `QueueSingle` still drains correctly
  (no double-drain, no leaked slots).
- Confirm a failing/panicking event handler still releases its slot (no permanent leak that would wedge
  the pod at capacity).

## Noted, not in scope

`QueueSingle` **coalesce-drops on the event path** (`max_allowed = 1`, the `_ => 1` arm above) instead of
queueing — unlike the cron/manual path where it genuinely queues via `on_queue` + drain. A real asymmetry:
a graph configured to queue silently drops events instead. Worth its own card if it bites.
