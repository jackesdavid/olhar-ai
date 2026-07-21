# Workflow node memoize — off by default + Variable panel read-only — Bug

**Board card:** #130 (Bugs)
**Branch:** `fix/workflow-node-memoize-readonly` (off `main`, deployed independently of the Tuya feature)
**Goal:** New workflow nodes must default to memoize=OFF (matching what the editor shows and what
Rust `Graph::add_node` does), and Get/Set Variable's name + type must be read-only in the panel.

## Symptom
`Test Loop` (Default tenant) logs `memoize on for a node on a loop — it won't iterate` for its
`get_variable`/`set_variable` nodes, even though the editor shows their Memoize toggle OFF — so the
loop's variable read/write don't iterate.

## Root cause
The editor's `addNodeKind` pushes a new node WITHOUT a `memoize` field. On graph Save,
`save_graph` deserializes `Vec<GraphNode>` and `#[serde(default = "default_true")]` fills the
missing field with **true**. So an untouched node silently persists as memoize=ON, while the UI
(`selectedNode.memoize === true` → `undefined` = off) shows it OFF. Confirmed in prod: Test Loop
Variable nodes 22/23/24/26 stored `memoize:true`. Rust `Graph::add_node` already defaults false —
the frontend just never mirrored it. `default_true` stays only for genuine pre-memoize back-compat.

## Fix (frontend-only — deploy `sentinel-app`, no control/flow change)
1. `addNodeKind` pushes `memoize: false` explicitly → saves preserve OFF, matching Rust + the UI.
2. Get/Set Variable panel: render `variable` + `type` as **read-only** (disabled Input) — they're
   owned by the Variables panel; editing them desyncs the node's dynamic port.

## Production data fix (one-off)
Set `memoize=false` on the `Test Loop` graph's `get_variable`/`set_variable` nodes (prod `workflows`
table, Default tenant) so the already-broken loop iterates without a re-save.

## Not doing (blast-radius avoidance)
Not flipping the backend `default_true` → keeps existing graphs of other tenants that were saved
without the field (ancient) behaving as before. Only the frontend now writes explicit `false`.

## Out of scope
Wiring the Memoize toggle to the immediate-persist endpoint (`setNodeMemoize`) instead of the
deferred graph-Save — separate UX cleanup.
