# Workflows engine (`sentinel-flow`)

Graph-based automation: a trigger fires a DAG of typed, single-responsibility nodes that runs
once to completion. The pure domain + engine live in `crates/app-core/src/workflows/`
(`domain.rs`, `engine.rs`, `node.rs`, `repo.rs`); `sentinel-flow` is the always-on runtime
(poller + executor + SSE bus); the editor is `sentinel-app/src/routes/workflows/+page.svelte`
(catalog-driven from control's `GET /workflows/catalog`). Node reference:
[docs/workflow-nodes.md](../workflow-nodes.md).

## Engine

`run_dag_with` → `run_driver` runs **multi-epoch forward Kahn passes** over the ordering edges
(exec back-edges excluded), settling each node once per epoch. Features:
- **Per-node memoize** (`GraphNode.memoize`): reuse cached output on re-activation, or re-run.
  Loop bodies / live sources (`GetCurrentFrame`) want memoize=off. **Event nodes never
  memoize** (no-op signals) — `add_node` defaults them off + the inspector hides the toggle.
  (The `PUT /workflows/:id/nodes/:nid/memoize` route was added — it had been missing, so the
  toggle silently 404'd.)
- **Loops**: exec back-edges allowed (data cycles rejected); bounded by `WF_MAX_STEPS`
  (10_000) + `WF_MAX_RUNTIME` (60s). `ForEach` is the concrete sync loop.
- **Per-node IO timeouts** (`node::node_timeout`) wrap the `run_node` chokepoint.

## Triggers

`Manual` and `Cron { every_secs }` only. **`OnNewAlert` was removed** — alert reactivity is
meant to return as a future `System-Alerts` cross-graph event. The poller (`spawn_poller`)
evaluates Cron; manual fires come from `POST /runs`.

## Cross-graph events (tenant event bus)

`EmitEvent` / `OnEvent` nodes carry an event name in `config.event`. When an `EmitEvent`
fires, `sentinel-flow::fire_event(tenant, EventSource, event, emitting_run_id)` **fans out to
every ENABLED graph in the tenant** with a matching `OnEvent`, spawning each handler subtree
as a decoupled run — **coalesce-dropped per `(tenant, listener_graph, event)`** (skip if that
listener's handler is already running). The emitter logs a `dispatch`; each listener logs
`trigger` (by which graph) or `skip`. `fire_system_event` is the (unused) seam for future
`System-*` emitters. Events live in a per-tenant **registry** (`tenant_events` +
`tenant_event_categories`); the editor palette synthesizes per-event "On event: Foo" /
"Emit event: Foo" entries grouped by category, and new events are registered from the node
property field (name + category). Event nodes show their bound event name in the node body
(Unreal-style), and event-node headers are tinted (crimson) so they stand out.

## Run history + visibility

`workflow_runs` (trigger_source manual|cron|event, source graph + event for event-runs,
`pod_name`, status, started/ended, summary) + `workflow_run_logs` (per-run signals; per-node
lines only when `graph.debug`). Live `GET /runs` carries run_id/trigger/pod; `GET
/runs/history` + `/runs/:id/logs` back the list-view "Recent runs" drawer. **pod_name** =
`FLOW_INSTANCE_NAME` → `HOSTNAME` → `"flow"`. `mark_orphaned_runs(pod)` reconciles crashed
runs at boot. SSE event names: `workflow-run-start|run|node|log|event|print|image`.

## Camera binding (stable id, not name)

Camera nodes (`CameraSnapshot`, `GetCurrentFrame`) store the **stable device id** (ONVIF
`urn:uuid`, falling back to ip), NOT the name — so renaming a camera doesn't silently
re-point a node. `device_actions::resolve_device` matches **id → ip → name** (the last for
back-compat + the chat agent). The editor dropdown stores the id and shows the current name.

## Multi-tenant + ports

One pod runs all tenants (`WfStore = Vec<(tenant, Graph)>`), keyed `(tenant, graph_id)`.
Each run builds a `ToolContext` with the tenant's devices/alerts/beacons freshly loaded and a
frame source bound to the tenant's edge go2rtc (`EdgeFrameSource` over the tailnet box host).
Editor catalog auto-derives from `NodeKind::ALL` — adding a node kind needs `domain.rs` (enum
+ ALL + slug/label/category/ports/default_config) + a `run_node` arm; no `agent/tools.rs`
change (that's the chat agent, not workflow nodes).
