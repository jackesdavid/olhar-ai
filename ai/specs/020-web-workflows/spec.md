# Spec: 020 — Web migration: Workflows (graph-based automation)

> **Status: IN PROGRESS — web migration unit (M2 finisher).** Ports the ratatui 013 engine
> (`domain`/`node`/`engine`) into `app/crates/core/workflows` with path-parameterized persistence
> and the web `ToolContext`; the ratatui editor (`handler`/`view`) is replaced by Tauri commands +
> a Svelte node editor. Triggers/concurrency run in a backend poller. Ratatui `src/` untouched.

## Goal

The operator builds named automations as graphs of typed, single-responsibility nodes, sets each
graph's trigger (Manual / Cron / OnNewAlert) and concurrency policy, and the engine runs them
fire-and-forget — wiring cameras, alerts, vision, and beacons together without code.

## User Value

No-code automation over every migrated capability: "on any new warn+ alert, snapshot the front
camera, describe it, and notify my phone" becomes a graph. The headline power feature, on the web.

## Requirements

- Copy `src/workflows/{domain,node,engine}` into `app/crates/core/src/workflows`. Keep the typed
  ports, atomic node catalog, `Graph` validation (typed edges · single-fed inputs · no cycle),
  topological `run_dag`, and the `WorkflowEngine` (per-graph trigger timers + `ConcurrencyPolicy`
  counters + new-alert high-water mark) **verbatim in logic**. Drop the ratatui editor state
  (`WfMode`/`WorkflowsState`/viewport). Persistence is **path-parameterized** → app-data
  `workflows.json`.
- **Node executors use the web `ToolContext`** (devices + app-data paths + resolved Telegram
  config + the UI-effect channel), so a snapshot streams to the UI (`chat-image`-style event)
  and alert mutations emit `alerts-changed`, consistent with unit 019.
- **Managed state**: the `Workflows` collection + a `WorkflowEngine`, both behind `Arc<Mutex>`.
  Graph edits go through commands so Rust stays authoritative for ids + typed-edge validation.
- **Commands**: `load_graphs`; `node_catalog` (static kind/port metadata for the palette + UI type
  checks); `create_graph`/`rename_graph`/`delete_graph`; `set_graph_trigger`/`set_graph_concurrency`
  /`set_graph_enabled`; `add_node`/`remove_node`/`move_node`/`set_node_config`; `add_edge`
  (returns the validation error on reject) /`remove_edge`; `run_graph` (Run-now, respects policy).
- **Engine poller**: a background thread (started at setup) ticking ~1 s — fires due Cron graphs
  and `OnNewAlert` graphs (ignoring `source=="workflow"` alerts to avoid loops), applying each
  graph's `ConcurrencyPolicy` (spawn / queue / skip), running the DAG on a blocking task, and
  emitting run events. Seeded from the current alert max at boot so the backlog doesn't fire.
- **Editor** (`/workflows`): a graph switcher (create/rename/delete/select); a canvas of the
  selected graph's nodes with typed input/output ports; a palette to add nodes; wire-up that
  rejects type-mismatched / already-fed / cyclic links (via `add_edge`); a config panel for the
  selected node's literals + the graph's trigger/concurrency/enabled; Run-now; run feedback.

## Acceptance Criteria

- [ ] `load_graphs`/`save` round-trip named graphs with triggers, concurrency, nodes, typed edges.
- [ ] Build `get_last_alert → format → notify_beacon`; `add_edge` rejects an `Image→Text` wire and
      a second wire into a fed input; Run-now executes the DAG in topo order.
- [ ] A `Cron{every_secs}` enabled graph fires on the poller; an `OnNewAlert{min_level}` graph fires
      when a qualifying non-workflow alert arrives; concurrency policy skips/queues as configured.
- [ ] `cargo check` + `npm run build` clean; core tests pass; ratatui `src/` untouched.

## Dependencies

- 019 Chat (the web `ToolContext` + tool events), 016 Alerts, 017 Notifications, 014 Devices, 018 Map.

## Risks

- Editor complexity (typed-port wiring) is the main cost — keep node layout simple, lean on the
  backend for all validation.
- The poller spawns runs on background tasks; a small global in-flight cap backstops runaways.

## Notes

Completes **M2 — Spatial + AI**. The ratatui `handler.rs`/`view.rs` editor is intentionally not
ported; the Svelte editor is the web UX for the same model.
