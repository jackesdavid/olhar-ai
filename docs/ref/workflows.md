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

## Gated events: inputs that filter delivery (card: edge tracking)

**Standing decision: a new event-driven feature is a new *event* + gating inputs on `OnEvent`,
never a new node kind.** This was tried once (a `Track` node with `entered`/`exited`/`crossed`
exec-branch arms) and reverted — it needed new engine dispatch (matching a *set* of event kinds
to one node, routing to a branch, seeding per-branch ports) that duplicates what `OnEvent` +
the event registry already do generically. Extend the mechanism below instead.

**The mechanism.** `OnEvent` can carry extra, config-derived **input** ports for a specific
event — `GraphNode::input_ports`'s `on_event_input_ports(config)` (mirrors `payload_ports` for
its dynamic *output* ports). Today: `camera` (`PortType::Camera`) and `region`
(`PortType::Region`), present only when `config.event` is `object_entered`/`object_exited`/
`line_crossed`. These aren't just displayed — they **gate whether the node fires at all**:

- `Graph::resolve_camera_selector(node)` — the node's `camera` input's current value: the wired
  `GetCameraByName`'s `config.camera`, or (no wire) the node's own `config.camera` literal. Same
  wire-overrides-config pattern as `ObjectDetection`'s `class` input.
- `Graph::resolve_region(node)` — the wired `Region` node's `name`+`points` (no literal fallback
  — there's no sensible way to hand-type a polygon). `Region` is a pure, config-only node (like
  `GetCameraByName`) whose whole job is holding reusable geometry; wire the SAME `Region` node
  into several `OnEvent`s so they share one definition instead of each re-drawing it. Whether its
  points mean a line or a zone is decided by the *reading* node, purely by count (2 = line,
  `line_crossed`; 3+ = zone, `object_entered`/`object_exited`) — `Region` itself doesn't know or
  care which.
- `on_event_node(graph, event, event_camera, devices)` (`engine.rs`) is the actual gate: no
  camera on the event (e.g. `telegram_message`) → no filtering. Node unscoped (no camera
  resolved) → matches any camera (today's behavior, unchanged). Node scoped → the event's own
  camera (`payload["camera"]`) must resolve to the SAME device. `sentinel-flow::fire_event` is
  the one caller — it loads the tenant's devices once per fire and passes them through.

**Extending this for the next scoping need** (e.g. gating by `class`, or a future zone-adjacent
concept): add the input port in `on_event_input_ports`, a `Graph::resolve_*` helper beside
`resolve_camera_selector`/`resolve_region`, and one more condition in `on_event_node`'s match —
not a new node kind, not new dispatch plumbing.

**Query counterpart.** `GetTrackedObjects` is the pull-based twin of the entered/exited/crossed
*events* — "what's tracked right now" (an action node, camera in → `TrackedObjectArray` out,
`GET {edge_api}/tracks/{ip}`) instead of "something just changed". Reach for this pattern
(event for transitions + an on-demand `GetX` node for current state) before inventing a
stateful/polling node kind for a "check on my own schedule" need.

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
change (that's the chat agent, not workflow nodes). An edge-facing action node (camera in,
network call out) reaches the tenant's edge via `resolve_edge_api_base(sys)` (tailnet, port
8080 — `recording.rs`; reused by `GetTrackedObjects`), NOT the `frames`/go2rtc port, which is
snapshot-only. If the new kind is really "react to X" rather than "fetch/do X", see **Gated
events** above before reaching for a new node kind at all.
