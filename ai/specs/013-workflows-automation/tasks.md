# Tasks — Workflows (Graph-Based Automation)

Phased per the architecture. Each phase ships with unit tests; the existing suite must stay
green and no test writes disk (`if cfg!(test) { return }`).

## Phase 1 — domain.rs (types + named graphs) — ✅ DONE (12 tests green)
- [x] `PortType`, `Port`; `NodeKind` enum + helpers (`label`, `input_ports`, `output_ports`,
      `default_config`, `ALL`) — `config_fields` deferred to Phase 4 (editor concern)
- [x] `GraphNode`, typed `Edge{from,from_port,to,to_port}`, `Trigger`, `ConcurrencyPolicy`
- [x] `Graph` ops (`node`, `children`, `roots`, `topo_order`, `add_node`, `remove_node`
      cascade, `add_edge` with port-exists + type-match + not-already-fed + no-cycle + no-self-loop
      validation, `remove_edge`)
- [x] `Workflows` named-graph collection (create/delete/select) + persistence
      (`workflows.json`, test-guarded) — `rename` + per-graph viewport deferred to Phase 4 (editor)
- [x] Tests: add/remove cascade, `topo_order`, fan-in ordering, edge validation (type mismatch /
      double-fed / cycle / self-loop / unknown-port), JSON round-trip (graphs + trigger +
      concurrency + typed edges)

## Phase 2 — node.rs (isolation boundary + ports) — ✅ DONE (3 tests green)
- [x] `run_node(kind, &config, inputs, sys: &ToolContext) -> PortValues` — **DEVIATION**: adapts
      the underlying domain fns directly (`alerts::actions::create/deliver`,
      `notification::dispatch`, `device::actions::{resolve_device,capture_snapshot_bytes,decode_luma}`,
      `agent::vision::describe_image`) instead of the JSON `ToolRegistry` → typed in/out, no
      string-result parsing (`create` returns the `Alert` with its id). See node.rs header.
- [x] Typed `PortValue {Text,Num,Bool,Image}` — **DEVIATION** from `Value` so `Image` carries raw
      JPEG bytes; `get_last_alert` source; `literal_for` (config literal → typed value, empty=none)
- [x] Tests: `literal_for` coercion, `format` substitution, required-input error before any IO
- [~] IO nodes (`camera_snapshot`/`describe_image`/`deliver_alert`/`notify_beacon`) implemented but
      exercised at runtime (Tester phase), not offline — they need camera/Anthropic/Telegram and
      `alerts::actions` writes `alerts.json` (no test guard), so unit tests avoid them on purpose

## Phase 3 — engine.rs (per-graph triggers + topo run) — ✅ DONE (6 tests green)
- [x] `WorkflowEngine::new(alert_seen_max)` + per-graph running/queued counters + cron timers
- [x] Trigger/concurrency decision logic: `decide` (Spawn/Queue/Skip per `ConcurrencyPolicy`),
      `cron_due(now)`, `new_alert_fires` (`id > seen_max`, ignore `source=="workflow"`, ≥min_level,
      advance high-water), `on_spawn/on_queue/on_finish` (queue drain)
- [x] `run_dag` (topological order, run-once per node, edge/literal input resolution, stop-on-error)
- [x] Tests: `get_last_alert → format` wired+topo, missing-required-input stop (no IO), all three
      concurrency policies, new-alert watcher filtering + high-water advance
- [~] App-level glue — the poller that iterates graphs each tick + `spawn_run` thread +
      `RunResult` channel — deferred to **Phase 6** (App wiring), where the Arc stores / ui_tx exist;
      cron wall-clock timing verified at runtime there

## Phase 4 — handler.rs (editor: switcher + port linking) — ✅ DONE (10 tests green)
- [x] `WorkflowsState` (in `domain.rs`, mirrors `MapState`) + graph switcher
      (create/rename/delete/select); trigger/enabled/concurrency edited via `Config`
- [x] Modes `WfMode {Browse,Palette,Move,Link,Config,Pan,Graphs}` + selection; node add/delete/move; Run-now
- [x] Port-aware Link (source out-port → input-port; `add_edge` rejects mismatch/fed/cycle into `status`)
- [x] Config form: literal per field + enum selectors (`level`/`trigger`/`concurrency` via ←/→)
- [x] Tests: drop/link/bad-link-error/toggle/delete-clamp/config-edit/trigger-cycle/graph-CRUD/run-request/pan-zoom

## Phase 5 — view.rs — ✅ DONE
- [x] Canvas: edges (`CanvasLine`) → node boxes (`CanvasRect`) → labels + config summary (y-flip like map)
- [x] Graph list panel, palette, config/trigger form, status panel, empty-state
- [~] Visual appearance verified by compile + map-pattern parity; not yet eyeballed in a run (Tester phase)

## Phase 6 — app wiring — ✅ DONE
- [x] `mod workflows;`, `Screen::Workflows`, `App` fields `workflows` + `engine` + `workflow_tx/rx:(u32,String)`
- [x] `App::new` seeds `WorkflowEngine` from max alert id; `on_tick` → `poll_workflows()`;
      `Screen::Workflows` key arm (per-mode, mirrors `Screen::Map`)
- [x] `poll_workflows` (drain results + QueueSingle drain; `drain_new_alert_levels` once/tick;
      per-graph `cron_due`/`levels_qualify` → `decide` → spawn/queue/skip)
- [x] `spawn_run(graph_id)` (clone graph + Arc stores, build `ToolContext`, thread, `activity` guard,
      `(id,status)` back) — mirrors `deliver_selected_alert`
- [x] view dispatch + WORKFLOWS menu item
- [~] Note: Run-now (`R`) currently bypasses concurrency (always spawns) — fine for a manual test button
- [~] Note: workflow viewport is transient (no `workflowsview.json`) — deliberate; spec didn't require it

## Verification
- [x] `cargo build --no-default-features` (no warnings) + `cargo test --no-default-features --bin bishop`
      → **73 passed** (63 prior + 10 editor), existing suite unaffected
- [x] App runtime (Tester): WORKFLOWS reachable; canvas renders in TTY with port-accurate
      connectors; **mismatched `Image→Text` link rejected** in the UI (Test C) ✓
- [x] **Test A PASS** — Run-now (`R`) on a `create_alert` graph creates an alert visible on the
      ALERTS screen ⇒ live `spawn_run → run_dag → node → store` path proven end-to-end ✓
- [x] UX fixes found+fixed during the Tester pass: Link-mode port picker panel (`render_link`);
      adaptive grid (visible at every zoom); ■ input / ● output markers + cyan port-to-port edges
- [~] Optional remaining runtime checks: cron auto-fire interval (Test B); `OnNewAlert` offline
      `get_last_alert → create_alert` (Test E); `workflows.json` round-trips across restart (Test D)
- [~] `cargo build` with DEFAULT features (audio+crt, incl. the 1600×1000 CRT window) — verify on dev machine
