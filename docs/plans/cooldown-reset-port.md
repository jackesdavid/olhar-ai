# Cooldown Reset Port — Implementation Plan

**Board card:** Cooldown node: `reset` exec port
**Goal:** Let a workflow clear its own cooldown after a recording (or any operation) finishes,
so the next event triggers immediately — without introducing a new node or changing the engine scheduler.

## How cooldown works today

`engine.rs` handles Cooldown directly (it is not routed through `run_node()`).  
State stored in `workflow_node_state` as `{ "last_fired_at": <unix_ms> }`.  
`domain.rs` declares `NO_PORTS` for Cooldown — the engine owns all routing.

## The change

Add a second exec input port: **`reset`**.

When `reset` fires (instead of `exec_in`), the engine clears `workflow_node_state` for that
node and produces **no exec output** — the run stops there.  
When `exec_in` fires, existing cooldown logic runs unchanged.

The user wires `stop_recording(exec_out) → cooldown(reset)` — a back-edge the engine already
supports (multi-epoch scheduler).

```
on_event(motion) → cooldown(exec_in) → start_recording → wait 12s → stop_recording
                        ↑                                                   |
                        └──────────────── reset ◄──────────────────────────┘
```

## Phase 1 — Domain + repo

**`crates/app-core/src/workflows/domain.rs`**
- Change Cooldown input ports from `NO_PORTS` to:
  ```rust
  const COOLDOWN_IN: [Port; 2] = [
      p("exec_in", PortType::Exec, false),  // optional — either port can fire
      p("reset",   PortType::Exec, false),
  ];
  ```
- Update `NodeKind::Cooldown => inputs_outputs(&COOLDOWN_IN, &NO_PORTS)` in `ports()`.

**`crates/app-core/src/workflows/repo.rs`**
- Add `clear_node_state(db, tenant_id, graph_id, node_id)` — a simple DELETE:
  ```sql
  DELETE FROM workflow_node_state
  WHERE tenant_id = $1 AND graph_id = $2 AND node_id = $3
  ```

## Phase 2 — Engine handler

**`crates/app-core/src/workflows/engine.rs`**  
In the `NodeKind::Cooldown` branch, detect which exec port triggered the node.
The engine accumulates inputs for each node from resolved edges. Check:

```rust
let via_reset = node_inputs.contains_key("reset");
if via_reset {
    repo::clear_node_state(&sys.db, sys.tenant_id, graph.id, nid);
    on_log(nid, "Cooldown: reset — window cleared", 0);
    // No exec_out → execution stops here (back-edge is consumed)
    continue; // or return, depending on engine loop structure
}
// ... existing exec_in / window_secs logic unchanged
```

## Phase 3 — Deploy

- `task cloud-deploy -- sentinel-flow` (app-core change lives in sentinel-flow)
- No migration needed (`workflow_node_state` table already exists; clear_node_state is a DELETE)
- No frontend changes — the port auto-appears in the graph editor from the domain declaration

## Out of scope (v1)

- Resetting *all* cooldowns in a graph at once
- Cooldown exposing elapsed time as a wirable output
- Visual indicator that the cooldown was reset mid-run
