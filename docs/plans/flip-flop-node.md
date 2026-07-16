# Utility Flow Nodes — Implementation Plan

**Board card:** Flip Flop workflow node (ToDo)
**Nodes covered:** Flip Flop · Compare (integer comparison)

---

## Node 1 — Flip Flop

**Goal:** A flow-control node that alternates its exec output between `A` and `B` on each run — identical semantics to Unreal Engine's Flip Flop node, with cross-run persistence via the DB.

### Behavior

Each time the node is reached it fires one exec branch and flips for next time:

```
Run 1 → A fires   (is_a = true)
Run 2 → B fires   (is_a = false)
Run 3 → A fires   (is_a = true)
…
```

- **No inputs, no config.** Exec-driven: `exec_in` arrives, one branch leaves.
- **Exec outputs**: `a` and `b` (like `If` has `true`/`false`).
- **Data output**: `is_a` (bool) — which branch just fired, wirable to `If` or `Print`.
- **State is cross-run**, keyed by `(tenant_id, graph_id, node_id)` — persists between cron ticks.
- **Engine-level node** (like `If`/`ForEach`) — the engine reads/writes state and routes the exec branch. `run_node` is a no-op pass-through.

### Canonical use case

```
Trigger: Cron { every_secs: 60 }, Concurrency: SkipIfRunning

[Motion Watch] → If(moved) →true→ [Flip Flop]
                                        ├─ a → [Create alert] "Motion started"
                                        └─ b → [Create alert] "Motion still active"
```

---

## Node 2 — Compare (integer)

**Goal:** A pure data node that compares two numbers with a configurable operator and outputs a `result` bool — feeds directly into `If`. Either or both sides can be wired from upstream or typed as a literal in the inspector.

### Behavior

- **Inputs**: `a` (Num, optional), `b` (Num, optional)
- **Output**: `result` (Bool)
- **Config**: `operator` (string, one of `>` `>=` `<` `<=` `==` `!=`, default `==`), `a` (default `"0"`), `b` (default `"0"`)
- When a port is not wired the engine uses `literal_for_named` to fill it from config, so typing `5` in the inspector for `b` and not wiring it just works — no special handling in `run_node`.
- **Pure node** — no timeout, no DB, no engine-level handling. Lives fully in `run_node`.

### Canonical use cases

```
[Person detection] → (count) a → [Compare] operator=">" b=0 → (result) → [If] →true [Notify]

[RecordPresence] → ... → [Compare] a=count operator=">=" b=5 → [If] →true [Create alert]

[For each](index) → a → [Compare] operator="==" b=2 → (result) → [If] →true [Print]
```

---

## Implementation

### Phase 1 — DB migration (Flip Flop only)

File: `crates/app-core/migrations/0023_workflow_node_state.sql`

```sql
CREATE TABLE IF NOT EXISTS workflow_node_state (
    tenant_id  UUID    NOT NULL,
    graph_id   INTEGER NOT NULL,
    node_id    INTEGER NOT NULL,
    state_json TEXT    NOT NULL DEFAULT '{}',
    PRIMARY KEY (tenant_id, graph_id, node_id)
);
```

Keyed by `(tenant_id, graph_id, node_id)`. `state_json` is a TEXT column (not JSONB — sqlx runtime-only, same pattern as other tables). The flip-flop stores `{"is_a": false}` (false = A fires next, toggled after each run).

---

### Phase 2 — repo helpers (Flip Flop only)

File: `crates/app-core/src/workflows/repo.rs`

```rust
/// Returns the persisted JSON state for a node, or `None` if never set.
pub fn get_node_state(db: &Db, tenant_id: Uuid, graph_id: u32, node_id: u32) -> Option<Value>

/// Upserts the JSON state for a node (best-effort, like `save_cron_fired`).
pub fn set_node_state(db: &Db, tenant_id: Uuid, graph_id: u32, node_id: u32, state: &Value)
```

Same pattern as `load_cron_state` / `save_cron_fired` — `block_on` a `tokio::runtime::Handle`, best-effort on error.

---

### Phase 3 — domain.rs (both nodes)

File: `crates/app-core/src/workflows/domain.rs`

**New variants:**

```rust
/// Flow control: alternates exec between `a` and `b` on each graph run (cross-run state).
/// Like Unreal's Flip Flop. `is_a` (bool data output) tells which branch just fired.
FlipFlop,

/// Data: compare two numbers with a configurable operator → `result` bool.
/// Both operands can be wired or set as literals in the inspector.
/// Feeds directly into `If` for conditional routing.
Compare,
```

**Port consts to add:**

```rust
static EXEC_OUT_FF: [PortDecl; 2] = [
    p("a", PortType::Exec, false),
    p("b", PortType::Exec, false),
];
static FF_OUT: [PortDecl; 1] = [p("is_a", PortType::Bool, false)];

static COMPARE_IN: [PortDecl; 2] = [
    p("a", PortType::Num, false),
    p("b", PortType::Num, false),
];
static COMPARE_OUT: [PortDecl; 1] = [p("result", PortType::Bool, false)];
```

**Catalog wiring** — bump `NodeKind::ALL` to 26 and add to each match arm:

| Arm | FlipFlop | Compare |
|---|---|---|
| `slug` | `"flip_flop"` | `"compare"` |
| `label` | `"Flip flop"` | `"Compare"` |
| `category` | `"Flow"` | `"Flow"` |
| `exec_out_ports` | `&EXEC_OUT_FF` | standard `&EXEC_OUT` |
| `input_ports` | `&NO_PORTS` | `&COMPARE_IN` |
| `output_ports` | `&FF_OUT` | `&COMPARE_OUT` |
| `default_config` | `json!({})` | `json!({ "operator": "==", "a": "0", "b": "0" })` |

---

### Phase 4 — engine.rs (Flip Flop only)

File: `crates/app-core/src/workflows/engine.rs`

Inside `forward_epoch`, handle `FlipFlop` at the same level as `If` — before `run_node_timed` is called. When the node is active:

1. Read `repo::get_node_state(db, tenant_id, graph.id, nid)` → parse `is_a: bool` (default `false` → **A fires first**).
2. Toggle for next time: `next_is_a = !is_a`.
3. Write `repo::set_node_state(...)` with `{"is_a": next_is_a}`.
4. Route exec: emit `(nid, "a")` into `live_out` if `is_a`, else `(nid, "b")`.
5. Store `is_a` (current value, not next) as data in `outputs[nid]` so the `is_a` bool port is wirable.
6. Mark node settled — skip the `run_node` call.

The engine already owns `sys.db`, `sys.tenant_id`, and `graph.id` — no signature changes.

**Memoize guard:** if a `FlipFlop` node has `memoize = true`, treat it as `false` (it's a side-effecting node, must re-run on every reach).

---

### Phase 5 — node.rs (both nodes)

File: `crates/app-core/src/workflows/node.rs`

**`run_node` arms:**

```rust
// Engine-driven (like EmitEvent/ShowImage) — never called directly:
NodeKind::FlipFlop => Ok(PortValues::new()),

// Pure comparison:
NodeKind::Compare => run_compare(config, inputs),
```

**`run_compare` implementation:**

```rust
fn run_compare(config: &Value, inputs: &PortValues) -> Result<PortValues, String> {
    let a = inputs.get("a").and_then(PortValue::as_num).unwrap_or(0);
    let b = inputs.get("b").and_then(PortValue::as_num).unwrap_or(0);
    let op = config.get("operator").and_then(Value::as_str).unwrap_or("==");
    let result = match op {
        ">"  => a > b,
        ">=" => a >= b,
        "<"  => a < b,
        "<=" => a <= b,
        "==" => a == b,
        "!=" => a != b,
        _    => return Err(format!("unknown operator: {op}")),
    };
    let mut o = PortValues::new();
    o.insert("result".into(), PortValue::Bool(result));
    Ok(o)
}
```

**`node_timeout` arms** — both go in the pure/instant list (returns `None`):

```rust
| NodeKind::FlipFlop
| NodeKind::Compare => return None,
```

**`debug_summary` arms:**

```rust
NodeKind::FlipFlop => "Flip Flop".into(),
NodeKind::Compare => {
    let op = config.get("operator").and_then(Value::as_str).unwrap_or("==");
    format!("a {op} b")
},
```

---

### Phase 6 — Inspector dropdown (Compare)

File: `sentinel-app/src/routes/workflows/+page.svelte`

Add a `{:else if key === "operator"}` branch in the config field renderer (same pattern as `key === "level"` and `key === "category"`):

```svelte
{:else if key === "operator"}
  {@const ops = [">", ">=", "<", "<=", "==", "!="]}
  {@const opv = String(selectedNode.config[key] ?? "==")}
  <Select.Root type="single" disabled={editLocked} value={opv}
    onValueChange={(v) => saveNodeConfig(selectedNode, { ...selectedNode.config, [key]: v ?? "==" })}>
    <Select.Trigger class="h-8 w-full font-mono">{opv}</Select.Trigger>
    <Select.Content>
      {#each ops as op (op)}
        <Select.Item value={op} label={op} class="font-mono">{op}</Select.Item>
      {/each}
    </Select.Content>
  </Select.Root>
```

This branch renders for any node that has `operator` in its config keys — currently only `Compare`, but it's reusable if we add other comparison nodes later.

---

### Phase 7 — docs

File: `docs/workflow-nodes.md`

Add two rows to the **Flow control** table:

| **Flip flop** | — | exec: `a` / `b`; `is_a` (bool) | — | Alternates exec between `a` and `b` on each graph run (cross-run persistent). `is_a` tells which branch just fired. | `Motion Watch → If(moved) → Flip flop →a Create alert("started") →b Create alert("still active")` |
| **Compare** | `a` (num), `b` (num) | `result` (bool) | `operator` (`>` `>=` `<` `<=` `==` `!=`), `a` (default 0), `b` (default 0) | Compares two numbers; `result` feeds `If`. Either operand can be wired or typed as a literal in the inspector. | `Person detection → (count) a → Compare(operator=">", b=0) → (result) → If →true Notify` |

---

## Open decisions

1. **Flip Flop — reset?** Add a `reset` bool input port to force back to A? Defer to v2.
2. **Flip Flop — memoize within a run?** If the node fires multiple times in one run (inside a loop), it toggles on each reach — this matches UE behavior but may surprise users. Document clearly.
3. **Compare — float support?** Currently integer only (`i64`). Add a separate `Compare (float)` node later if needed, or extend this one with a `mode` config (`integer` / `float`). Defer.
4. **Compare — string comparison?** `==` and `!=` on text would be useful (e.g. compare person name). Defer — separate `Text equals` node is cleaner.

## Out of scope (v1)

- Flip Flop reset input port
- Float / text comparison variants
- Counter node (natural companion — increment/decrement a persistent count)
- Gate node (open/close an exec path)
