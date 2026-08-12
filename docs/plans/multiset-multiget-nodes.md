# Set Variables / Get Variables (multi) — Implementation Plan

**Board card:** #179 Set Variables / Get Variables — one node instead of a column of them
**Goal:** One node that writes N graph variables, and one that reads N graph variables, so a graph
stops growing a vertical column of single-variable nodes beside every event and every sink.

## Why

The face pipeline is the proof. Unpacking `face_detected` today means **eight** `Set Variable`
nodes stacked down the canvas, each with one exec hop and one wire; feeding `Store Face` means
**eight** `Get Variable` nodes stacked back up. Sixteen nodes and thirty-two wires to move eight
values across a graph. The nodes are correct — there are just far too many of them, and the shape
of the graph stops describing what the graph does.

These two are **convenience nodes, not new semantics**. They read and write the same per-run
`ToolContext::variables` map (`crates/app-core/src/workflows/node/variable.rs`) that
`GetVariable`/`SetVariable` already use, so a `Set Variables` here and a plain `Get Variable`
there interoperate exactly as you would expect. Nothing about the engine changes.

## The shape

**Set Variables** — exec in, exec out (like `SetVariable`), plus **one typed input per variable
listed in its config**. Add `bbox`, `confidence`, `face_width`, `blur`, `yaw`, `pitch`, `roll`,
`image` and the node grows eight inputs, correctly typed, in that order.

**Get Variables** — pure, no exec (like `GetVariable`), plus **one typed output per variable
listed in its config**.

The list is edited in the right-hand properties panel: an **Add variable** picker showing the
graph's declared variables that are not already on the node, and a row per chosen variable with
its type and a remove button.

## Precedent to copy, not invent

`DictMakeRow` (#113) already does exactly this — a config array becomes N typed ports:

- Backend derivation: `dict_row_ports()` in `crates/app-core/src/workflows/domain.rs:241`,
  dispatched from `GraphNode::input_ports()`.
- Client mirror: the `dict_make_row` branch of `effectiveKind()` in
  `sentinel-app/src/routes/workflows/[[id]]/+page.svelte:1166`.
- Panel editor: the Dictionary Schema columns editor,
  `schemaColumns()` / `setSchemaColumns()` around `+page.svelte:2905`.

Follow those three, in those three places. The type vocabulary comes from `VariableDecl`
(`domain.rs:2479`), so no `cell_type_to_port`-style mapping is needed — a variable's `ty` is
already a `PortType`.

## Phase 1 — the node kinds

- `NodeKind::MultiSetVariable` + `NodeKind::MultiGetVariable` in
  `crates/app-core/src/workflows/domain.rs`: add to the enum, to `NodeKind::ALL`, to `slug()`
  (`multi_set_variable` / `multi_get_variable`), `title()` ("Set Variables" / "Get Variables")
  and `category()` → `"Variables"`.
- Exec ports: `MultiSetVariable` takes exec in/out exactly like `SetVariable`;
  `MultiGetVariable` is pure — add it beside `GetVariable` in the dead-branch exemption list
  (`domain.rs:1656`) so a pure read never marks a branch dead.
- Static port tables (`domain.rs:1730` / `:1819`): both are `&NO_PORTS`, like Get/Set — every
  port is derived.

## Phase 2 — derived ports

- `fn multi_variable_ports(config: &Value, required: bool) -> Vec<ResolvedPort>` in `domain.rs`,
  next to `variable_port()`. Reads `config.variables` (`[{ name, type }]`), skips blank names,
  and emits one `ResolvedPort` per entry. `required: false` on both sides — see decision 4.
- Dispatch: `MultiSetVariable` from `GraphNode::input_ports()`, `MultiGetVariable` from
  `output_ports()`.
- Client mirror in `effectiveKind()`, right beside the `dict_make_row` branch.

## Phase 3 — execution

In `crates/app-core/src/workflows/node/variable.rs`:

- `run_multi_set_variables(config, inputs, sys)` — for each configured variable, if the
  same-named input is present, write it into `sys.variables`. **A missing input writes nothing**
  (decision 4). Returns empty `PortValues`; exec passes through.
- `run_multi_get_variables(config, sys)` — for each configured variable, emit the stored value or
  `zero_value(ty)` under the variable's name, reusing the existing helper verbatim.
- Dispatch from `node.rs:605`; add both to the pure/no-exec list at `node.rs:718`.
- Run-log summary at `node.rs:1248`: `set $a, $b, $c (+2)` rather than the generic fallback — a
  node that writes eight things must say which eight in the log.

## Phase 4 — the editor

- `MANAGED_KEYS` (`+page.svelte:1078`): add `multi_set_variable: ["variables"]` and
  `multi_get_variable: ["variables"]`, or the generic config form renders the raw JSON array.
- Properties panel section: the Add-variable picker + the chosen list. Source the options from
  `graph.variables`, minus the ones already on the node.
- **Removing a variable from the list must drop its edges.** A port that no longer exists with a
  wire still attached is a graph that fails validation at run time and looks fine on screen.
- House style: no logic in the template — named helpers returning bool/string
  (`nodeVariables(node)`, `availableVariables(node)`).

## Phase 5 — variable rename and retype (the trap)

Port **names here are variable names**, which single Get/Set nodes never had to care about —
their port is always called `value`. So:

- **Rename a graph variable** → every multi node's `config.variables[].name` must be rewritten
  **and** every edge's `from_port` / `to_port` touching that node must be rewritten in the same
  pass. Miss the edges and the rename silently unwires the graph. `repo::rename_recipient`
  (`crates/app-core/src/workflows/repo.rs`, #139) is the shape to follow — same problem, solved
  once already.
- **Retype a graph variable** → the node's baked type goes stale (see decision 3).

Cover both with a test over a graph holding one multi node and a wired edge per port.

## Phase 6 — docs

- `docs/workflow-nodes.md`: both nodes, in the Variables section, stating plainly that they are
  sugar over the singles and share the same run map.
- `docs/ref/workflows.md`: one line under variables.

No migration. Both kinds are additive and their config is JSON; old graphs are untouched.

## Open decisions

1. **Two kinds or one with a direction flag?** Proposed: **two**. The ports differ in direction,
   and the add-node palette is a menu someone reads — "Set Variables" and "Get Variables" are
   what they will search for.
2. **Auto-populate on add?** Proposed: **no**. Dropping the node lists nothing; you add what you
   want. A node that silently grows a port every time someone declares a variable is a node that
   rewires graphs behind your back.
3. **Bake the type into the node, or re-derive from the graph declaration each time?** Get/Set
   bake (`config.type`), so consistency says bake — and baking is what keeps a wire's type stable
   when the declaration is deleted. But a stale type on a multi node is stale N times over.
   Proposed: bake, plus a drift marker in the panel and a one-click resync. **Decide before
   Phase 2** — it sets the config shape.
4. **A configured variable whose input is unwired: skip, or write the zero?** Proposed: **skip**.
   "Not wired" means "don't touch this one", which is what lets the same node be reused on two
   branches that each set a different subset. Writing zeros would clobber values set upstream.
5. Reordering rows — drag, or up/down buttons? Proposed: **insertion order only in v1**; remove
   and re-add to reorder.

## Out of scope (v1)

- Set-from-a-list / loop-driven writes — that is `ForEach` plus a single `Set`.
- Cross-graph or persisted variables. These stay per-run, like the singles.
- Any change to `GetVariable`/`SetVariable`. They remain, and remain the right node for one value.
- Auto-migrating an existing column of single nodes into one multi node. Tempting, and a separate
  card if it turns out to be wanted.
