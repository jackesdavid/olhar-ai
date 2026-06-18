# Events Carry Typed Payload Data — Implementation Plan

**Board card:** Events carry typed payload data (In Progress)
**Goal:** An event defines **any number of typed fields** (e.g. Text "Message", Num "Score"). Configuring an `EmitEvent` for that event gives it one **input port per field**; the matching `OnEvent` exposes one **output port per field** — wireable in the same or another graph.

> **Foundational.** [[motion-detection-node-v2]] emits an event carrying its result. Land this first.

## Locked decisions
- **Any number of fields** per event (`[{name, type}]`), stored as **jsonb** on the registry.
- **Bake the schema onto the node config.** When a node's `event` is chosen, copy the event's `fields` into `node.config.payload`. The `app-core` domain layer has no event registry, so config-derived ports keep it self-contained (the frontend re-bakes when the event/schema changes — staleness handled at edit time).
- **Value-flow is edge-driven**; only port *declaration* is dynamic. EmitEvent's payload = its gathered data inputs; OnEvent's outputs = the delivered payload. So the engine needs node-aware ports at exactly two spots (gather + edge-validate), not a new value channel.

## Phase 1 — Registry schema (migration + repo + control)
- `crates/app-core/migrations/0021_event_payload.sql`: `ALTER TABLE tenant_events ADD COLUMN payload jsonb NOT NULL DEFAULT '[]'`. Holds `[{name, type}]` (type = a `PortType` slug). Existing rows → `[]` (payload-less), no break.
- `TenantEvent` (`app-core/src/workflows/repo.rs:202`) gains `fields: Vec<EventField>` (`{name:String, ty:PortType}`); `create_event` writes the jsonb, `list_events` reads it.
- Control `create_event`/`list_events` (`sentinel-control/src/domain/workflows.rs:226,242`) accept + return `fields`.

## Phase 2 — Config-derived ports (domain core)
- New `ResolvedPort { name: String, ty: PortType, required: bool }` + `payload_ports(config)` reading `config.payload`.
- `GraphNode::input_ports()/output_ports()/input_port()/output_port()` (owned, node-aware): `EmitEvent` → payload as **inputs**, `OnEvent` → payload as **outputs**, every other kind → its static `kind.*_ports()`.
- `Graph::add_edge` (`domain.rs:776`): resolve port types from the **node** (config-aware) not just the kind, so a data edge into `EmitEvent.<field>` / out of `OnEvent.<field>` validates.

## Phase 3 — Engine value-flow (app-core engine)
- Input gather (`engine.rs:276`): iterate `n.input_ports()` (node-aware) instead of `n.kind.input_ports()`; refactor `literal_for(config, Port)` → `literal_for_named(config, name, ty)` (payload ports have no config literal → wires only).
- `emit` callback `FnMut(&str)` → `FnMut(&str, &PortValues)`; at EmitEvent fire (`:378`) pass `&inputs` as the payload.
- Thread an `entry_payload: &PortValues` through `run_driver`/`forward_epoch`; when the forced `OnEvent` entry runs, set its output = the payload (downstream reads each field by port name). `run_subpath_eventful` gains the payload param; normal runs pass empty.

## Phase 4 — Cross-graph delivery (sentinel-flow)
- `fire_event` / `spawn_event_handler` / `execute_run` (`sentinel-flow/src/engine.rs`) thread the `PortValues` payload to `run_subpath_eventful`. The emit closure passes the EmitEvent's inputs; `fire_system_event` passes empty. (Same-pod only — cross-pod is [[multi-pod-flow-event-fabric]].)

## Phase 5 — Frontend
- Event registration dialog (`+page.svelte:2171-2209`): a repeatable **fields** editor (name + PortType dropdown; zero fields = payload-less).
- On saving an `EmitEvent`/`OnEvent` node's `event`, bake `config.payload` from the selected event's `fields` (the `events` list is already loaded via `loadEvents()`).
- Node rendering: compute EmitEvent/OnEvent ports from `config.payload` (overlay on the catalog's empty inputs/outputs) so the extra ports draw + wire. Keep markup logic in named helpers; `tools.rs` unchanged (not a workflow-node list).

## Open decisions (resolved / remaining)
1. ✅ Many fields (jsonb). ✅ Bake-onto-node. ✅ Edge-driven value-flow.
2. **Type-change safety:** if an event's `fields` change after graphs wired it, baked node ports + edges can go stale/mismatched. v1: re-bake on event re-select; surface mismatched edges via existing typed-edge validation. (A "sync ports" action is a follow-up.)
3. **Field name collisions / reserved names** (e.g. clashing with `exec`): validate field names on registry save (non-empty, unique, not `exec`).

## Out of scope (v1)
- Literal (un-wired) defaults for payload inputs — payload travels by wire only.
- Cross-pod payload delivery ([[multi-pod-flow-event-fabric]]).
- Payload on system/non-graph emitters (`fire_system_event` sends empty).
