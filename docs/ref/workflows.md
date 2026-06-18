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
fires, `sentinel-flow::fire_event(tenant, EventSource, event, emitting_run_id, payload)` **fans
out to every ENABLED graph in the tenant** with a matching `OnEvent`, spawning each handler
subtree as a decoupled run — **coalesce-dropped per `(tenant, listener_graph, event)`** (skip
if that listener's handler is already running). The emitter logs a `dispatch`; each listener
logs `trigger` (by which graph) or `skip`. Event-node headers are tinted (crimson) and show the
bound name (`Emit event: Foo`).

**Typed payloads.** An event may declare any number of typed fields (`{name, type}`). An
`EmitEvent` for it gets one **input port** per field; the matching `OnEvent` exposes one
**output port** per field (same/other graph). Ports are **config-derived** — the schema is
baked onto `node.config.payload` when the event is chosen; `GraphNode::input_ports/
output_ports` resolve them (`EmitEvent`→inputs, `OnEvent`→outputs), `add_edge` validates
against them, and `run_subpath_eventful` **seeds the OnEvent entry's outputs** with the
delivered payload so downstream nodes read each field by name. Value-flow is edge-driven, so
only port *declaration* is dynamic. (Frontend: a fields editor on registration; the open graph
re-bakes payloads on open via `reconcileEventNodes`.)

**System events.** `repo::system_events()` is a **code-defined catalog** of platform events
(currently `motion`, with payload `camera/score/area/blobs/bbox`), merged into control's
`list_events` for every tenant (system wins on a name clash; read-only in the editor). So
`OnEvent("motion")` is available — with typed ports — before any event fires, and shipping a
new system event reaches all tenants on deploy. **`fire_system_event` is now live**: edge
motion arrives via control → Redis `flow:inject` → flow's injector → `fire_event(System,
"motion", payload)`. Per-graph sensitivity is a cloud-side `If score >= …`. The Redis fabric
also makes the fan-out + the SSE relay multi-pod (see [architecture ref](architecture.md)).

Events live in a per-tenant **registry** (`tenant_events` + `tenant_event_categories`) plus the
system catalog; the editor palette synthesizes per-event "On event: Foo" / "Emit event: Foo"
entries grouped by category.

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
