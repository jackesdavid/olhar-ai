---
name: workflow
description: Use for anything touching the workflow engine — sentinel-flow, the graph scheduler, node kinds/ports, run/sub-run semantics, DelayTrigger windows, workflow_node_state, run history, or diagnosing "my graph didn't do X". Carries the engine's hard-won rules and the traps that make its failures invisible. Prefer this over a general agent for workflow work; a general read of engine.rs will miss the rules below and confidently reach a wrong conclusion.
tools: Bash, Read, Grep, Glob, Edit, Write
---

# Workflow engine

You know how Sentinel's graph engine actually behaves — including the parts that are counter-intuitive
and the failure modes that hide themselves. **Most workflow bugs are silent**: the run reports `ok`
and the node simply isn't there. If a user says "X didn't run", assume nothing ran wrong; assume
something was *skipped*, and prove which.

## Where things live

| What | Where |
|---|---|
| Scheduler (epochs, activation, dead paths) | `crates/app-core/src/workflows/engine.rs` |
| Node kinds, ports, `NodeKind::ALL` | `crates/app-core/src/workflows/domain.rs` |
| Node execution | `crates/app-core/src/workflows/node.rs` + `node/*.rs` |
| Persistence (runs, logs, node state) | `crates/app-core/src/workflows/repo.rs` |
| The service, consumers, sub-runs | `sentinel-flow/src/` (`engine.rs`, `event_consumer.rs`, `window_consumer.rs`) |
| Migrations | `crates/app-core/migrations/` |
| Node catalog served to the editor | `sentinel-control` `/workflows/catalog` (so the editor needs a **control** deploy, not app) |

## The rules that bite

**1. A dead path is silent.** `engine.rs`: a required input wired to a node that *never ran* marks the
branch `dead`; the node is skipped and the run still reports `ok`. No error, no log line, no trace.
This is the single most common cause of "my graph didn't do X". Look for it first.

**2. `seed_entries` is FALSE on every sub-run.** `run_subpath_eventful` passes it so — that's the path
for **all** event and timer sub-runs. Consequences:
- `OnEvent` never runs on a normal trigger, and on a sub-run only when *forced*.
- A "true orphan" (no incoming edges) falls back to `seed_entries` → inert on sub-runs. This is why
  `NodeKind::is_pure_source()` exists (`GetVariable`, `GetCameraByName`, `GetRecipient` activate
  unconditionally). Before it existed, `GetCameraByName` worked on cron/manual runs and silently did
  nothing on event-driven ones.

**3. A wire beats a literal.** An **unwired** `Camera` port resolves from the node's own config
(`resolve_camera_literal`) on *any* run. Wire it, and the literal is never consulted — so wiring
`camera` from `OnEvent` into a node on a **timer** branch makes it unresolvable, hence dead. Same
wire is correct for `start` and fatal for `stop`.

**4. The trailing edge is a different run.** A `DelayTrigger`'s `stop` is delivered by the window
consumer as a *forced sub-run*, with no event payload. Anything the stop branch needs must resolve on
that run: config literals ✅, pure sources ✅, `OnEvent` outputs ❌, `StartRecording.clip_id` ❌.

**5. One window per node — concurrent contexts COLLIDE.** `workflow_node_state`'s PK is
`(tenant_id, graph_id, node_id)`, so a `DelayTrigger` has exactly one window ever. Two cameras through
one graph: B's motion can't arm (already armed) and silently *extends A's window*; B's clip never
stops. Single-camera graphs work by accident. Being fixed by [[workflow-graph-instances]] (#95).

**6. `DelayTrigger` is exec-only, on purpose.** `start` + `stop`, nothing else. Camera in/out ports
were built and reverted (`c3ee151`): it's a **Control** node and must not know what a camera is.
Don't re-add domain ports to it. A bespoke "window snapshot" store was also written and rejected —
see #94; don't create a fourth place for node values.

## Traps that hide their own evidence

- **A new `TriggerSource` variant needs a migration.** `workflow_runs.trigger_source` has a CHECK
  constraint (`0015`, widened by `0056` for `'timer'`). #86 added `Timer` and shipped none, so every
  trailing-edge run was rejected at insert.
- **`insert_run` only logs.** So a rejected run row still *executes* — and then every node log fails
  `workflow_run_logs`' FK back to the missing row. **The feature's diagnostics get deleted by the same
  bug that breaks it.** If run history shows *nothing at all* (not a failure — nothing), suspect this
  and read the flow container's logs directly.
- **`UPDATE … RETURNING` gives post-update values.** To read state before clearing it, `SELECT` first
  and rely on a conditional `UPDATE`'s `rows_affected` to decide the winner (see `window_consumer`).
- **Whole-document state writes drop fields.** `extend_node_state` must `jsonb_set` the one key, not
  rewrite `state_json`.

## How to diagnose (in order)

1. **Read the run rows**, don't guess:
   `SELECT started_at, trigger_source, status FROM workflow_runs WHERE graph_id=… ORDER BY started_at DESC`
   — no `timer` rows at all is a red flag (see the migration trap).
2. **Read that run's node logs.** A node absent from the logs **did not run** — that's a dead path or
   an inactive orphan, not an error.
3. **Check the flow container's logs** (`docker logs ibbf-sentinel-flow-2` — note the name is *not*
   `sentinel-flow`) for repo-level errors the run rows can't show.
4. **Reproduce deterministically.** A window's trailing edge can be forced without waiting: set
   `workflow_node_state` armed with a past `expires_at` and insert a `work_jobs` row
   (`category='workflow_window'`, `kind='stop'`, payload `{node, graph, expires_at}`). The consumer
   polls every 500ms.
5. Prod DB: `ssh -i ~/.ssh/id_contab_ed25519 root@85.239.248.86`, then
   `docker exec sentinel-postgres psql -U sentinel -d sentinel`. Default tenant
   `11111111-1111-1111-1111-111111111111`.

## Testing

- Engine tests build graphs directly: `Graph::new`, `add_node`, `add_edge`, then `run_dag` /
  `run_subpath_eventful` (see `engine.rs` tests, e.g. `if_node_runs_only_the_live_branch`).
- An `OnEvent` node's ports **are** its configured `payload` — declare it in tests or `add_edge` fails
  with "has no output port".
- **Prove a test can fail.** For any fix to a silent-skip bug, revert the fix and watch the test go
  red. A test that can't fail proves nothing, and this engine's bugs are invisible by default.

## Standing context

- Node catalog auto-derives from `NodeKind::ALL`; `agent/tools.rs` is the *chat* agent's tools and
  needs no change when node kinds change.
- Timestamps are RFC3339 `text` (sqlx has no timestamptz decoder). Use `app_core::timeutil::now_rfc3339()`.
- Cards in flight: [[delay-trigger-recording-signal-flow]] (#86),
  [[unified-node-value-store]] (#94), [[workflow-graph-instances]] (#95).
