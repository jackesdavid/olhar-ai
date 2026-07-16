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

## What was built (option b, not a — reasoning)

The plan leaned toward **(a)** "make event runs call `on_spawn`/`on_finish`". Reading the code at
implementation time ruled that out — it was the wrong call for two reasons:

1. **`on_finish` is coupled to the queue.** It decrements `queued` and returns "drain me"
   (`engine.rs:158-161`). An event run calling it would silently swallow a queued `QueueSingle` cron fire.
2. **`decide()` reads the same counter** for per-graph policy. Feeding event runs into `runs.running` would
   make a cron fire get skipped because an *event handler* is running — a behaviour change well beyond the bug.

**(b) is surgical and strictly correct.** `state.activity` is the one map *every* run registers with —
`mark_run_started`/`mark_run_finished` are called by both `spawn_graph_run` and `spawn_event_handler`. So:

- New `sentinel_flow::engine::total_in_flight(&Activity)` sums it, and **both** backstop sites read it —
  the event fan-out (`engine.rs:~459`) *and* the cron poller (`engine.rs:~56`), which undercounted for the
  same reason.
- **`WorkflowEngine::total_running()` was deleted**, not just bypassed. Leaving a helper that silently omits
  every event run is exactly the trap that caused this bug; a comment in its place explains why.
- **Race fixed too:** `mark_run_started` used to run *inside* the spawned thread, so the dispatch loop could
  check the ceiling, spawn, and check again before the thread registered — overshooting the cap precisely
  during a storm. Both spawn paths now register **before** `thread::spawn`, matching how `event_runs` and
  the old `on_spawn` reserve synchronously.
- Tests: `sentinel-flow`'s first unit tests (`total_in_flight_counts_event_runs_not_just_cron`,
  `total_in_flight_ignores_settled_graphs`).

Counters now have clear owners: `WfEngine.runs` = per-graph policy accounting (cron/manual, feeds `decide`);
`state.event_runs` = per-`(tenant, graph, event)` handler cap; `state.activity` = live in-flight, pod-wide —
the UI badge **and** the global ceiling.

## Known limitation (not introduced here)

If a run panics outside a node, `mark_run_finished` never runs and `activity.running` leaks — which now
wedges the ceiling, not just a UI badge. The intended design had the same hole (`on_finish` sits at the end
of the same closure), and node panics are already contained by `run_node_timed`'s channel-disconnect path.
An RAII guard around the decrement would close it properly.

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
