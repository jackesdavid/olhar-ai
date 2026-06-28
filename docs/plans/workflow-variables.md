# Workflow Variables — Implementation Plan

**Board card:** #56 Workflow Variables — typed vars + Get/Set nodes (ToDo)
**Goal:** Declare typed, per-workflow variables in a left-hand panel (Unreal-style); each
variable spawns two **data-only** nodes — **Get \<name\>** and **Set \<name\>** — whose ports are
typed by the variable, so values flow across the canvas without long repeated wires.

## Context

Today, to reuse a value (a camera, a count, a flag) in several places you must wire from the
same source node again and again, cluttering the graph. Variables let you **set once and get
anywhere**. This mirrors Unreal Blueprint's variables panel, and maps almost exactly onto the
machinery we already have for **event fields** (the typed name+type dropdown) and **dynamic
ports** (how `Emit Event` / `On Event` derive instance ports from their config).

## Reuse — what already exists (don't rebuild)

- **Type dropdown:** `PAYLOAD_TYPES` in `sentinel-app/src/routes/workflows/+page.svelte` (the 11
  selectable wire types) + the add/remove field-row UI used for event fields (`newEventFields`).
- **Dynamic per-instance ports:** `GraphNode::input_ports()/output_ports()` in
  `crates/app-core/src/workflows/domain.rs` special-case `EmitEvent`/`OnEvent` and build ports from
  config via `payload_ports(&config)` → `ResolvedPort { name, ty, required }`. The editor mirror is
  `effectiveKind()` / `payloadFields()` in `+page.svelte`.
- **Data-only (pure) nodes:** `NodeKind::is_pure()` (domain.rs) — pure kinds emit no exec pins; the
  catalog endpoint (`sentinel-control/src/domain/workflows.rs::catalog`) omits exec pins when pure,
  and the editor mirrors a `PURE_KINDS` set in `+page.svelte`. Models: `GetCameraByName`,
  `GetRecipient`, `GetLastAlert`, `BreakCamera`.
- **Graph persistence:** `Graph` is a JSON blob in `workflows(tenant_id, id, data)`
  (`crates/app-core/src/workflows/repo.rs` load/save). Adding a `Graph` field is additive — **no
  migration** (use `#[serde(default)]`).
- **Node add flow:** `Graph::add_node()` (domain.rs) + `addNodeKind()` (+page.svelte) + the
  `POST .../nodes` endpoint (workflows.rs). Per CLAUDE.md, the catalog auto-derives from
  `NodeKind::ALL`; `agent/tools.rs` does NOT list workflow nodes, so no change there.

---

## Phase 1 — Backend: data model + Get/Set node kinds

**Files:** `crates/app-core/src/workflows/domain.rs`, `node.rs` (+ new `node/variable.rs`).

1. **Variable declaration on the graph:**
   ```rust
   pub struct VariableDecl { pub name: String, #[serde(rename = "type")] pub ty: PortType }
   // on Graph:
   #[serde(default)] pub variables: Vec<VariableDecl>,
   ```
2. **Two new kinds** in `NodeKind` + `NodeKind::ALL`: `GetVariable`, `SetVariable`.
   - `slug()`: `"get_variable"` / `"set_variable"`; `label()`: `"Get Variable"` / `"Set Variable"`;
     `category()`: `"Variables"`.
   - `default_config()`: `json!({ "variable": "", "type": "text" })` — the variable **name** and its
     **type baked into config** (same approach as `EmitEvent` baking `payload`), so port resolution
     is self-contained and needs no graph lookup.
   - `node_timeout()`: `return None` (instant, no IO).
3. **Dynamic ports** — extend the `GraphNode::input_ports()/output_ports()` match (next to
   `EmitEvent`/`OnEvent`) with a `variable_port(&config)` helper that reads `config.type` → one
   `ResolvedPort { name: <var or "value">, ty, required }`:
   - `GetVariable` → **output_ports** = `[variable_port]`, no inputs.
   - `SetVariable` → **input_ports** = `[variable_port]`, no outputs.
4. **Pure / exec:** `GetVariable` is `is_pure() = true` (data-only, no exec). `SetVariable` — see
   **Open decision 1** (recommended: a single exec passthrough `exec_in → exec_out`, not pure).
5. **Execution** (`node.rs` dispatch → `node/variable.rs`):
   - `run_set_variable(config, inputs, run_vars)` → writes `inputs["value"]` into the run's
     variable store keyed by name; passes exec through.
   - `run_get_variable(config, run_vars)` → returns `{ <name>: run_vars.get(name).cloned()
     .unwrap_or(<type-zero default>) }`.

## Phase 2 — Engine: per-run variable store

**Files:** `crates/app-core/src/workflows/engine.rs`, `node.rs` (`run_node` signature/threading),
`sentinel-flow` run context.

- Add a run-scoped `variables: HashMap<String, PortValue>` (shared `Arc<Mutex<…>>`), created per
  run in `build_run_context` and threaded to `run_set_variable`/`run_get_variable`. Scope =
  **one run** (local-variable semantics — see Open decision 2). Reuse the `seed_event_payload`
  upgrade path so Camera/Bbox values round-trip as typed values.

## Phase 3 — Persistence + CRUD for `graph.variables`

**Files:** `sentinel-control/src/domain/workflows.rs`, `sentinel-app/src/lib/data.ts`.

- `PUT /workflows/:id/variables` (body: `[{name,type}]`) via the existing `mutate()` flow (load →
  set `graph.variables` → save → poke engine). Add `data.setGraphVariables(id, vars)`.
- Round-trips for free in the JSON blob; renaming/removing a variable updates the declaration list
  (handling of referencing nodes → Open decision 3).

## Phase 4 — Frontend: the Variables panel + Get/Set nodes

**Files:** new `sentinel-app/src/routes/workflows/components/VariablesPanel.svelte`;
`+page.svelte`.

1. **Left panel** (mirrors the right inspector's resize/persist logic): a list of variable rows —
   **name input + type dropdown reusing `PAYLOAD_TYPES`** + remove; an "Add variable" button. Saves
   via `setGraphVariables`.
2. **Per-variable palette entries:** each declared variable renders **Get \<name\>** and
   **Set \<name\>** buttons that call `addNodeKind("get_variable"/"set_variable")` then set
   `config = { variable: name, type: <decl.type> }` (extend `addNodeKind`'s config-patch like the
   `presetEvent` case).
3. **Dynamic ports in the editor:** extend `effectiveKind()` to build the single typed port for
   `get_variable` (output) / `set_variable` (input) from `config.type`; add `get_variable` (and
   `set_variable` if pure) to the `PURE_KINDS` set.
4. **Hide the bare kinds from the generic palette** — `Get/Set Variable` are only meaningful per
   variable, so filter their slugs out of the default `NodePalette` (they're spawned from the
   Variables panel, like the synthesized per-event `On/Emit event: <name>` entries).
5. Node body shows the variable name (read `config.variable`) so Get/Set nodes are legible.

## Phase 5 — Polish

- Validate variable names (unique, non-empty, identifier-ish).
- On variable **type change / rename**, re-sync `config.type`/`config.variable` on referencing
  nodes (or warn). On **delete**, warn if Get/Set nodes reference it.
- Update `docs/workflow-nodes.md` with the two kinds + the Variables panel.

---

## Open decisions (grill before building)

1. **Set ordering — the crux.** With BOTH nodes data-only (no exec pins, no wire between them), the
   dataflow scheduler has no guaranteed ordering, so a `Get` could read before its `Set` ran. Unreal
   solves this by making **Get pure (data-only)** but **Set impure (one exec pin)** so Set sits in
   the control-flow chain at the exact point it should write. **Recommendation:** give `SetVariable`
   an exec passthrough (`exec_in → exec_out`) and keep `GetVariable` pure. This still meets the goal
   — reads need zero wires; Set just drops inline in the flow you already have. If you insist both be
   pure, ordering is best-effort (works for "set early / get late" but not guaranteed).
2. **Scope:** per-run (reset each run, like a local) vs persisted across runs (like a global/state).
   **Recommendation:** per-run for v1 (matches "set at the beginning of the flow"). Cross-run state
   is a separate feature.
3. **Rename/type-change propagation:** auto-resync referencing Get/Set nodes vs warn-and-leave.
   **Recommendation:** resync on save; warn on delete-while-referenced.
4. **Read-before-set default:** return the type's zero value (`""`, `0`, `false`, empty list) vs
   error. **Recommendation:** zero value (predictable, no run failures).

## Out of scope (v1)

- Cross-run / global persistent variables.
- Variable folders/categories, structs, arrays-of-variables beyond the existing array port types.
- Drag-and-drop placement (click-to-add from the panel is enough for v1).
