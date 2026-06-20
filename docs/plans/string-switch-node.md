# String Switch Node — Implementation Plan

**Board card:** String Switch node (ToDo)
**Goal:** A multi-arm switch node that routes exec flow to the arm whose label exactly matches an incoming string — user-defined N arms, configured in the inspector.

---

## Background

The workflow engine already special-cases `If`, `ForEach`, and `FlipFlop` in `engine.rs::forward_epoch` — those three nodes diverge from the default "fire all exec_out ports" path. `StringSwitch` follows the same pattern: only ONE exec arm fires, chosen at runtime by comparing the incoming value against the arm labels stored in `node.config`.

The critical design wrinkle: **all existing exec port arrays are `&'static [Port]`** (compile-time constants), but `StringSwitch` arms are user-defined. The event-payload nodes (`EmitEvent`/`OnEvent`) solve the analogous problem for DATA ports via `GraphNode::input_ports()`/`output_ports()` which read from `node.config` at runtime. We extend the same idea to exec output ports for `StringSwitch` only.

---

## Phase 1 — Backend: domain + node + engine

Files: `crates/app-core/src/workflows/domain.rs`, `node.rs`, `engine.rs`

### 1a. `domain.rs`

- Add `StringSwitch` to `NodeKind` enum and `NodeKind::ALL`.
- `slug`: `"string_switch"`, `label`: `"String Switch"`, `category`: `"Flow"`.
- **Input ports**: `value: Text` (required) — the string to match.
- **Exec out ports static declaration**: Return `&[]` (empty slice) from `exec_out_ports()` for `StringSwitch`. The actual arms are instance-level and stored in config — the engine and frontend bypass the static slice for this kind.
- **Default config**: `{"arms": ["arm_1", "arm_2"]}` — two arms by default.
- Add a helper on `GraphNode` (or a free function): `string_switch_arms(node: &GraphNode) -> Vec<String>` — reads `node.config["arms"]` as `Vec<String>`.

### 1b. `node.rs`

Add `run_node` match arm for `StringSwitch`:
- Read `value` from inputs (`as_text()`), return `Err` if missing.
- Return `Ok(PortValues::new())` — no data outputs; routing is engine-side.

### 1c. `engine.rs` — `forward_epoch` special case

In the `if n.kind == NodeKind::If { … }` block, add an `else if n.kind == NodeKind::StringSwitch` branch:
```rust
else if n.kind == NodeKind::StringSwitch {
    let val = inputs.get("value").and_then(PortValue::as_text).unwrap_or_default();
    let arms = string_switch_arms(n);        // reads config["arms"]
    if let Some(arm) = arms.iter().find(|a| a.as_str() == val) {
        live_out.insert((nid, arm.clone()));
    }
    // if no arm matches, nothing fires (dead path — all downstream nodes skip)
}
```
This fires exactly the matching arm, or nothing if the value doesn't match any label.

### 1d. `add_edge` validation

`add_edge` checks that an exec output port name is valid for the source node. Extend the check: for `StringSwitch`, validate the port name against `string_switch_arms(node)` instead of the static `exec_out_ports()`.

---

## Phase 2 — Frontend: instance-level exec port rendering

File: `sentinel-app/src/routes/workflows/+page.svelte`

The Svelte editor currently calls `execOutPorts(k: CatKind)` (catalog, kind-level). For `StringSwitch` the arms are per-node-instance.

**Pattern:**
- Add a helper `nodeExecOutPorts(n: GraphNode, k: CatKind): CatPort[]`:
  - If `n.kind === "string_switch"`, return `(n.config.arms ?? []).map(a => ({ name: a, type: "exec" }))`.
  - Otherwise, delegate to `execOutPorts(k)` (existing behaviour, unchanged).
- Replace calls to `execOutPorts(k)` in the node rendering loop with `nodeExecOutPorts(n, k)` when a node instance is available.
- The existing layout math (`outDataOffset`) already handles N exec outputs correctly — it offsets data ports by `execOutPorts.length - 1`.

---

## Phase 3 — Inspector: arms editor

File: `sentinel-app/src/routes/workflows/+page.svelte` (inspector panel)

For a `string_switch` node selected in the inspector, render an **arms list editor**:
- Show each arm as an editable text input with a remove (×) button.
- An "Add arm" button appends a new arm to `config.arms`.
- Changes call the existing `PUT /workflows/:id/nodes/:nid/config` (or the node-update path) to persist.
- On change, the node re-renders its exec output ports in the canvas.

UI behaviour:
- Minimum 1 arm enforced (remove disabled when only 1 remains).
- Arm labels are trimmed; empty labels are rejected.
- Changing a label name that already has a wired edge should warn the user that the edge will be disconnected (edge validation will fail on next save).

---

## Open decisions — resolve before building

1. **No-match behaviour**: If the incoming string doesn't match any arm, nothing fires. Should there be an optional **"default" arm** (like `switch ... default:`)? Defer to v2 unless the use-case clearly needs it.
2. **Case sensitivity**: Exact case-sensitive match in v1. Case-insensitive can be a config toggle later.
3. **Catalog exec ports**: Returning `&[]` for `exec_out_ports()` means the catalog API emits no exec output ports for this kind. The frontend must know to treat this node's exec outs as instance-derived (keyed by `n.kind === "string_switch"`). This is a deliberate deviation — document it.
4. **Edge disconnect on arm rename**: When a user renames an arm that has an outgoing edge, that edge becomes invalid. Decide: silent drop on save, or a UI warning before the rename commits.

---

## Out of scope (v1)

- Contains / starts_with / regex matching — exact string equality only.
- A "default" fallback arm.
- Emitting the matched arm label as a data output (easy add later).
- Cross-node arm synchronisation (e.g., renaming an arm across multiple nodes).
