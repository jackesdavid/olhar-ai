# Live instance view — see what's running, and each run's context

**Board card:** #102 Live instance view — running instances + per-context values (ToDo)
**Follows:** [[workflow-graph-instances]] (#95) — the engine now keys state/values per instance; this
is the UI that reads them. The two owner asks that #95's engine work enabled but never surfaced:
*"can we see, in some row, the values of each run context"* and *"see all current running instances
of a graph"*.

---

## Why this is a separate card

#95's engine half is done and verified (scope + per-instance values, live on the real box). But its
card title promised a "live instance view" that was never built — I marked #95 done for the bug it
fixed and left this feature invisible. Splitting it out keeps the board honest: the engine is done,
the view is not.

## What exists to build on

- **`workflow_runs`** already has `status='running'`, `graph_id`, `pod_name`, `started_at`, and
  `mark_orphaned_runs` reaps crashed pods' rows. A running instance ≈ an unfinished run row.
- **`workflow_node_state.values_json`** (0058) holds each node's last output per
  `(tenant, graph, node, instance_key)` — the per-context values, already persisted.
- **`instance_key`** (0057) is the context id (`""` = Single; the camera for PerCamera).

So the data is already there. This card is the read path + the surface — no new persistence.

## The surface

1. **A running-instances list** per graph: for each live instance (an unfinished run, or an armed
   `workflow_node_state` row), show its `instance_key` (the camera name for PerCamera), when it
   started, and what it's waiting on (an armed DelayTrigger window + its expiry).
2. **Open an instance → its value context**: the `values_json` for each node in that instance —
   literally "the values of each run context", node by node. Media renders from its MinIO URL once
   #94 lands; until then, scalars/camera/clip_id/bbox (what the codec persists today).
3. Live-ish: the editor already has an SSE events stream (`/events`); a coarse refresh or a
   `workflow-instance` event is enough — this is an inspector, not a dashboard.

## Open decisions

1. **Where it lives** — a panel in the workflow editor (per-graph, next to Runs), or a top-level
   "Instances" screen across graphs? Leaning: editor panel first (the context is one graph), a
   global view later if wanted.
2. **What counts as "running"** — only instances with a pending latent action (an armed window), or
   any run in flight? An ordinary run finishes in milliseconds, so the interesting case is the armed
   window. Leaning: show armed/waiting instances prominently; recent completed runs as history.
3. **Retention of the value context** — per-instance rows accumulate (esp. PerCamera). Sweep on
   instance end + a TTL; `0054`'s nightly `events` sweep is the precedent. (Also flagged in #95.)

## Verification

- A graph with an armed DelayTrigger shows one running instance; its context shows the camera and
  clip_id that armed it. Two cameras (PerCamera) → two instances, side by side, each with its own
  values.
- The window closing (or the run finishing) drops the instance from the live list.
- Opening an instance shows the same node values the trailing edge reads — they match.

## Out of scope

- The value store's media-to-MinIO move (#94) — this view reads whatever the store holds; when #94
  makes media a URL, media renders here for free.
- Changing instance semantics (#95) — this is read-only over what the engine already writes.
