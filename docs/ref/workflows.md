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

**Scheduling is a NODE property now, not a graph one.** A `When Event` node has a source selector;
in **Cron** mode it holds a `Schedule` (hourly/daily/weekly/monthly/yearly + IANA tz) and fires only
that node, so one graph can hold several independent schedules. `schedule_consumer.rs` keeps one
`work_jobs` row per cron node and reschedules it after each fire; `next_occurrence` does the
calendar math. The node renders a live countdown from a server-computed `next_fire_at` — one source
of truth for *when*, the client only formats it.

`Trigger` on the graph now has **one variant, `Manual`**. `Cron { every_secs }` (the 1 s interval
poller) and `Schedule(Schedule)` were removed in migration `0095`, along with `spawn_poller`,
`WorkflowEngine::cron_due`, the boot restore in `sentinel-flow/src/state.rs`, `load_cron_state` /
`save_cron_fired`, `PUT /workflows/:id/trigger` and the editor's trigger picker. Two ways to
schedule the same graph needed a precedence rule nobody remembered, and one production graph really
did run on both clocks at once.

What that cost, stated plainly: **sub-minute polling is gone.** `Cron { every_secs: 2 }` was legal
and a calendar's finest grain is the minute. A graph that genuinely needs it builds an exec loop with
a `Delay`; a graph that was polling for something (the two seeded templates that polled at 2 s and
3 s) should listen for the `motion` event instead, which is what they do now.

`Trigger` is kept as a one-variant enum rather than deleted: every stored graph's JSON carries
`"trigger":"Manual"`, and a future top-level trigger needs somewhere to go. The `Schedule` STRUCT is
very much alive — cron nodes hold one.

**`OnNewAlert` was removed** — alert reactivity is meant to return as a future `System-Alerts`
cross-graph event. Manual fires come from `POST /runs`.

The migration order is load-bearing: `0095` must land BEFORE the code that drops the variants, or a
row still holding `{"Cron": …}` fails to deserialize and the graph stops loading entirely — not
"loses its schedule". It also deletes the three seeded templates it rewrote so `seed_templates`
re-inserts the new bodies (they are `ON CONFLICT DO NOTHING` against a stable id).

## Device Gate — a human approves before the graph continues (#176)

A node that suspends a run, asks a person on their phone, and continues down **Positive** /
**Negative** / **Failure**. Agnostic: it gates an exec edge, not a device.

Three arms, not two, and the split carries weight: `negative` is an ANSWER — someone read it and
said no — while `failure` means NO HUMAN DECIDED (timed out, undeliverable, error). Collapsing them
is how a graph comes to treat "nobody answered" as "go ahead".

| Piece | Where |
|---|---|
| `device_gate_requests` table | migration `0094` |
| Request lifecycle (mint / settle / claim / pending) | `crates/app-core/src/workflows/device_gate.rs` |
| Node kind + 3 exec arms | `workflows/domain.rs` (`NodeKind::DeviceGate`) |
| Ask + resume | `workflows/engine.rs` (engine-driven, like `DelayTrigger`) |
| Push with a machine-readable payload | `notification/push.rs` (`send_data`) |
| Resume sweep | `sentinel-flow/src/gate_consumer.rs` |

Five rules that are load-bearing, each with a real failure behind it:

- **The run ENDS at the ask.** A run cannot block on a person — a handler runs to completion before
  the pass settles it. Same machinery as `DelayTrigger`'s trailing edge, so there is one suspend
  mechanism rather than two. A `waiting` run would also make a `SkipIfRunning` graph refuse every
  trigger while a phone lies face-down.
- **A settled request can never be settled again.** Every settle is ONE conditional `UPDATE … WHERE
  state = 'pending' RETURNING`; the row count answers "did I win". Do NOT copy password reset's
  check-then-act shape — benign there (both writes set the same password), a gate opened twice here.
- **The row IS the queue.** `resumed_at` makes delivery durable: control settles and nudges flow,
  but `FLOW_URL` is load-balanced DNS and that nudge can be lost. The sweep claims anything settled
  and undelivered, so an approval can never leave a graph silently unfinished.
- **The token never reaches anything that renders.** Stored as `sha256` hex, sent in the push `data`
  field. In `body` it would print a gate-opening secret on the lock screen.
- **A phone holds at most N unanswered requests** — `device_gate_max_pending` in `tenant_settings`,
  default 10, 1–100, edited on Settings → General. Depth, not rate: a rate limit still lets a loop
  bury someone overnight. At the cap nothing is minted and the node takes `failure` with the reason
  in the run log. The count and the insert share ONE transaction behind a per-device
  `pg_advisory_xact_lock` — count-then-insert is a check-then-act on exactly what a runaway loop
  does concurrently, and two asks at `cap - 1` would both write. Expired-but-unswept rows are not
  counted (`expires_at > now`), or a phone that ignored yesterday is full forever.

Decisions are in `docs/plans/device-gate-map/` (five resolved + the cap). Notable: only the
**addressed device** may answer; biometrics fall back to the **device passcode** with the factor
recorded; iOS **critical alerts are unobtainable** (Apple restricts them to health/public-safety),
so the TTL assumes the phone is CHECKED, not alerted — which is why `GET /device-gate/pending` is
the reliable path and the push is the optimisation.

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
Editor catalog auto-derives from `NodeKind::ALL` — adding a node kind needs `domain.rs` (enum
+ ALL + slug/label/category/ports/default_config) + a `run_node` arm; no `agent/tools.rs`
change (that's the chat agent, not workflow nodes). An edge-facing action node (camera in,
network call out) reaches the tenant's edge via `resolve_edge_api_base(sys)` (tailnet, port
snapshot-only. If the new kind is really "react to X" rather than "fetch/do X", see **Gated
events** above before reaching for a new node kind at all.

## Build status

**Device Gate (#176)** — branch `feat/device-gate-node`, pushed, no MR yet.

Done and verified: migration `0094` applied against a real Postgres with the full prior chain; all
seven `device_gate.rs` statements `PREPARE` and their return types match the Rust tuples; the
double-tap race proven closed (first settle returns 1 row, second returns 0). Workspace compiles.

`node_id` is **`bigint`**, matching `run_logs` and NOT the `integer` used by `workflow_node_state`.
Every call site binds `node_id as i64` and sqlx will not widen int4 into i64 — `integer` would
compile, pass review, and fail every read at runtime.

The per-phone cap is in and verified the same way: `PREPARE` on every new statement (the advisory
lock's parameter had to be `$1::uuid::text` — `$1::text` infers a TEXT parameter that a bound `Uuid`
cannot satisfy, and it compiles either way), the counting rule checked against seeded rows (3
pending count, an expired one and an answered one do not), and the lock proven to serialise two
sessions on the same device while leaving a different device free.

Not done: `POST /device-gate/:id/answer` and `GET /device-gate/pending` in control; the properties
panel + canvas countdown; the mobile approval screen (ticket 06, still open — it is a prototype).
**Nothing has run end to end** — no graph has entered the node and no phone has received a push.

**Graph-level `Trigger::Cron` / `Trigger::Schedule` are REMOVED** (same branch). Migration `0095`
rewrites every stored trigger to `"Manual"` and deletes the three seeded templates it rebuilt;
`alerta_pessoa` and `presenca_mapa` now listen for `motion` instead of polling at 2 s / 3 s, and
`relatorio_diario` carries a cron node (daily 08:00 America/Sao_Paulo). The migration was applied
against a real Postgres over the full prior chain and re-applied to prove it idempotent; a row with
`{"Cron":…}`, one with `{"Schedule":…}`, one already `"Manual"` and one with NO trigger key all end
up `"Manual"`.

`enabled` is deliberately untouched by the migration. `Turn On/Off Daily` (tenant `Default`) was the
only legacy trigger armed in production and it already had the two cron nodes (18:00 and 08:09) that
replaced it — it was running on two clocks, and now runs on the two it should. Disarming it to be
tidy would have taken a working automation down.
