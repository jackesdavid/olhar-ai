# Architecture Notes — Workflows (Graph-Based Automation)

New module `src/workflows/`, mirroring the `src/map/` vertical slice. Action nodes are thin
adapters over the **existing** agent tool registry / vision call — no new external IO.

## Components

```
src/workflows/
  mod.rs       re-exports
  domain.rs    PortType, Port, NodeKind, GraphNode, Edge, Graph, Trigger, ConcurrencyPolicy,
               Workflows (named-graph collection), load/save (workflows.json), WorkflowsState (editor)
  node.rs      isolation boundary: input_ports()/output_ports() per kind, run_node (kind→tool
               adapter), get_last_alert source, no Graph ever passed to a node
  engine.rs    WorkflowEngine: per-graph trigger polling + ConcurrencyPolicy, run_dag (topo +
               cycle guard + stop-on-error), RunResult channel, per-graph running/queued counters
  handler.rs   editor logic (graph switcher + modes Browse/Palette/Move/Link/Config/Pan), selection, edge ops
  view.rs      ratatui Canvas: graph list, port-to-port edges (CanvasLine) + node boxes
               (CanvasRect with port rows) + labels; palette; config/trigger form; status
```

### Node catalog (v1) — atomic, single-responsibility; no trigger nodes

| Node | Kind | Inputs | Outputs | Backed by (existing) |
|---|---|---|---|---|
| Get Last Alert (source) | `get_last_alert` | — | `id:Num, level:Text, message:Text, room:Text, source:Text, created_at:Text, ok:Bool` | reads `AlertStore` (latest) |
| Create Alert | `create_alert` | `message:Text*, room:Text` | `id:Num, result:Text, ok:Bool` | `create_alert` → `alerts::actions::create` |
| Deliver Alert | `deliver_alert` | `id:Num*` | `result:Text, ok:Bool` | `deliver_alert` → `alerts::actions::deliver` |
| Camera Snapshot | `camera_snapshot` | `camera:Text*` | `image:Image, result:Text, ok:Bool` | `get_camera_snapshot` |
| Describe Image | `describe_image` | `image:Image*` | `description:Text, ok:Bool` | `src/agent/vision.rs` (on piped bytes) |
| Notify Beacon | `notify_beacon` | `message:Text*, target:Text, image:Image` | `result:Text, ok:Bool` | `send_notification` → `notification::dispatch` |
| Format | `format` | `p1:Text, p2:Text, p3:Text` | `text:Text` | pure (config `template`, e.g. `"{p1} — {p2}"`) |

`*` = required. **Triggers are NOT nodes** — see `Trigger` below.

## APIs (reused, not new)

- `src/agent/tools.rs`: `ToolRegistry::run`, `ToolContext`, `ToolUiEvent`, tools
  `send_notification` / `create_alert` / `deliver_alert` / `get_camera_snapshot`.
- `src/agent/vision.rs` for `describe_image`.
- `src/alerts/{domain,actions}.rs`, `src/notification/handler.rs::dispatch`.

## Data model (`domain.rs`)

- `PortType { Text, Num, Bool, Image }`; `Port { name: &str, ty: PortType, required: bool }`.
- `NodeKind` (snake_case serde) + helpers: `label()`, `input_ports()`, `output_ports()`,
  `default_config()`, `config_fields()`, `ALL`.
- `GraphNode { id:u32, kind, x:i32, y:i32, config: Value }` — `config` holds static settings
  (`level, source, template, …`) **and literal fallbacks for unconnected input ports** (Text/
  Num/Bool only; never Image). `center()`/`rect()` (y-down grid like `MapRoom`).
- **`Edge { from:NodeId, from_port:String, to:NodeId, to_port:String }`** (typed, directed).
- `Trigger { Manual, Cron { every_secs:u64 }, OnNewAlert { min_level: AlertLevel } }`.
- `ConcurrencyPolicy { SkipIfRunning, QueueSingle { max_queued:u32 }, Concurrent { max_concurrent:u32 } }`.
- `Graph { id:u32, name:String, enabled:bool, trigger:Trigger, concurrency:ConcurrencyPolicy,
  next_id:u32, nodes:Vec<GraphNode>, edges:Vec<Edge> }` with `node()`, `children(id)`,
  `roots()` (no-input / source nodes), `topo_order()`, `add_node`/`remove_node` (cascade-drops
  edges), `add_edge` (validate: ports exist · types match · target input not already fed ·
  no cycle), `remove_edge`.
- `Workflows { graphs: Vec<Graph>, next_graph_id, selected }` — the **named-graph collection**;
  CRUD + select. `load`/`save` to `workflows.json` (one object holding `graphs`), mirroring
  `src/map/domain.rs` (`if cfg!(test) { return }`). Per-graph viewport persisted separately
  (sibling file keyed by graph id, like `mapview.json`).

## Port wiring & isolation (`node.rs`)

The shared mutable bag is **gone**. The engine resolves each node's inputs and hands a node
only its own `PortValues`; the node returns only its declared outputs. The `Graph` is never
passed to a node.

- `PortValues = HashMap<String, Value>` (one node's resolved port values; Image as bytes).
- Engine resolves each declared input port: **wired edge** → upstream node's stored output;
  else **config literal**; else if `required` → run error `missing input <port>`.
- `run_node(kind, &config, inputs: &PortValues, sys) -> Result<PortValues, String>`:
  `build_tool_input` maps `(kind, config, inputs)` → `(existing_tool_name, input Value)`; calls
  `sys.registry.run(name, &input, &sys.tool_ctx)`; parses the result string into the kind's
  output ports (e.g. `create_alert.id` parsed from "Raised alert #N"; `describe_image.description`
  = the vision text). `format` is pure (substitutes `{p1..p3}` into `template`).
- `get_last_alert(sys) -> PortValues` reads the latest `Alert` from the store into its outputs
  (`ok=false` when the store is empty).

## Engine (`engine.rs`)

- `WorkflowEngine { cron_last: HashMap<GraphId,Instant>, alert_seen_max:u64,
  per_graph: HashMap<GraphId,{running:u32, queued:u32}>, result_tx, result_rx }` — an `App`
  field beside `workflows: WorkflowsState`. Seed `alert_seen_max` at construction from the
  current store max (backlog doesn't fire on boot).
- `poll_triggers(&workflows, &alert_store, now)`: iterate **every** graph; for each `enabled`
  graph whose `Trigger` is due — `Cron` timer elapsed, or a new alert ≥ `min_level` (watch by
  id, all `id > alert_seen_max`; ignore `source == "workflow"`) — apply its `ConcurrencyPolicy`:
  spawn / increment `queued` (≤`max_queued`) / skip. On run finish, decrement `running` and, for
  `QueueSingle`, drain one queued fire.
- `run_dag(&graph, &sys) -> RunResult`: compute `topo_order()`; for each node in order, resolve
  its inputs (edges → upstream outputs in a per-run `HashMap<NodeId,PortValues>`, else config
  literals), `run_node`, store outputs; **each node runs at most once**; `visited` cycle guard;
  **stop-on-error**. All blocking IO here, on a background thread. Run state is discarded at end.
- `App::spawn_run(graph_id)` mirrors `send_to_bishop` / `deliver_selected_alert`: clone Arc
  stores (`scan.devices`, `notification`, `alert_store`) + `tool_ui_tx`, build a `ToolContext`
  and a fresh `ToolRegistry::new()` inside the thread, run the DAG, send `RunResult` (tagged
  with `graph_id`) back; hold an `activity.begin()` guard so PROCESSING lights. A small global
  in-flight cap backstops all graphs.

## Editor (`handler.rs` + `view.rs`)

Reuse the map's viewport/zoom/pan/fit, index-based `selected` cursor, amber `status`, and
mode-per-keymap.
- **Graph switcher** — a list of named graphs above the canvas: create / rename / delete /
  select-active; per-graph config (`Trigger` + `enabled` + `ConcurrencyPolicy`).
- **Modes:** Browse (`A`dd / `L`ink / `C`onfig / `M`ove / `X`del / `R`un-now / `E`nable /
  pan/zoom), Palette (cycle `NodeKind::ALL`, Enter drops center-screen), Move, **Link** (pick
  source node → output port → target node → **type-compatible** input port; reject mismatch /
  already-fed), Config (per-kind form), Pan.
- **Canvas draw order** (y-flip like `map/view.rs`): port-to-port edges as `CanvasLine` → node
  `CanvasRect`s with input ports (left) / output ports (right) labelled → kind label + 1-line
  config summary. Empty-state note like map.
- **Config form** reuses the alert `Draft` / beacon-rename pattern: a literal field per
  *unconnected* input port + static cfg from `config_fields()` (text buffers + enum ←/→
  selectors, e.g. `level` / `min_level` / concurrency mode); connected ports show
  `← node#<id>.<port>`.

## App wiring (mirror existing features)

- `src/main.rs`: `mod workflows;`
- `src/app/domain.rs`: `Screen::Workflows`; `App` fields `workflows: WorkflowsState`,
  `engine: WorkflowEngine`.
- `src/app/handler.rs`: build both in `App::new` (engine seeds `alert_seen_max`); `on_tick`
  adds `poll_workflows()` after `poll_alert_status()`; a `Screen::Workflows` key arm modeled on
  `Screen::Map`; editor action methods + `spawn_run`/`poll_workflows`.
- `src/app/view.rs`: `Screen::Workflows => render_workflows(...)` via `framed(...)`.
- `src/menu/domain.rs`: `MenuItem { label: "WORKFLOWS", blurb: "automation graphs & triggers",
  target: Screen::Workflows, ... }`.

## Important Decisions

- **Typed ports + edge mapping over a shared context** — nodes get only their declared inputs
  and return only their declared outputs; the engine owns edges. Stronger isolation than a
  shared bag, and per-node output storage removes same-kind key collisions.
- **Atomic, single-responsibility nodes** — `describe_camera` is decomposed into
  `camera_snapshot → describe_image`; `notify_beacon` takes a wired `image` instead of
  self-snapshotting. Behavior is composed by linking; a `format` node handles text composition.
- **Triggers are an engine/graph property, not nodes** — firing is the engine's job; a graph
  declares its `Trigger`. Removes the cron/alert_created trigger nodes.
- **Multiple named graphs** — `workflows.json` is a collection; each graph is one automation
  with its own trigger/enabled/concurrency.
- **Fire-and-forget, run-once topological execution** — a fire runs the whole DAG once and
  discards state; recurrence is engine polling. Conditionals/loops/branching → Phase 2.
- **Reuse the tool registry / vision** for all action IO (spec 006) and **mirror the MAP
  slice** for the editor (spec 009).

## Risks

- Action→trigger feedback loop — watcher **ignores `source=="workflow"`**; `create_alert`
  defaults `source="workflow"`; cycle guard in `run_dag`.
- `get_last_alert` race — returns the latest alert, not provably the firing one (v1 ok).
- Editor complexity — port linking + type-check + graph switcher; mitigate with index-based
  selection (no hit-testing) and a clear port list panel.
- Tick blocking — all IO on the run thread; per-graph `ConcurrencyPolicy` + global cap;
  `poll_workflows` only polls + drains channels.
- Image port values exist only in the in-memory per-run map (never persisted to
  `workflows.json`).
- Background tool UI events can pop a modal over any screen — acceptable v1; swap to a
  discard-sink if noisy.

## Implementation deviations (accepted during the Developer phase)

Two refinements were made while implementing and accepted by the operator:

1. **`PortValue` is a typed enum** (`Text/Num/Bool/Image`), not `serde_json::Value` — so an
   `Image` port carries raw JPEG `Vec<u8>`. `PortValues = HashMap<String, PortValue>`. Config
   literals stay `serde_json::Value` (Text/Num/Bool only) and `literal_for` coerces them.
2. **Action nodes adapt the underlying domain functions directly** — `alerts::actions::{create,
   deliver}`, `notification::dispatch`, `device::actions::{resolve_device, capture_snapshot_bytes,
   decode_luma}`, `agent::vision::describe_image` — instead of the JSON `ToolRegistry`. The node
   system handle is the existing `ToolContext` (`store/ui_tx/notif/alerts`). Reuse is still 100%
   existing code, one layer below the LLM tool surface; this gives typed in/out and avoids
   parsing tool result strings (e.g. `create` returns the `Alert` with its id).
