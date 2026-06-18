# Missing Comparison Nodes — Implementation Plan

**Board card:** Missing comparison nodes (ToDo)
**Goal:** Add TextCompare + boolean logic (And / Or / Not) nodes to complement the existing numeric Compare node.

## Context

The existing `Compare` node (`crates/app-core/src/workflows/domain.rs:183`, `node.rs:584`) compares
two `Num` inputs with `>`, `>=`, `<`, `<=`, `==`, `!=` → `Bool`. Missing from the toolkit:
- Comparing text values (equality, contains, prefix/suffix)
- Combining multiple Bool conditions (AND, OR, NOT)

## Phase 1 — TextCompare node (backend)

Files: `crates/app-core/src/workflows/domain.rs`, `node.rs`

- Add `TextCompare` to `NodeKind` enum
- Ports: `a: Text`, `b: Text` → `result: Bool`
- Config default: `{ "operator": "eq", "b": "" }` (b can be a wired port or a literal)
- Operators: `eq`, `ne`, `contains`, `starts_with`, `ends_with`
- `fn run_text_compare(config, inputs)` in `node.rs` — mirrors `run_compare` shape
- `ALL` constant: add `NodeKind::TextCompare` (bump array length to 28)
- Fill out `slug()` → `"text_compare"`, `label()` → `"Text Compare"`, `default_config()`,
  `data_inputs()`, `data_outputs()`, run summary in `run_summary()`

## Phase 2 — Boolean logic nodes (backend)

Files: same as Phase 1

- Add `NodeKind::And`, `NodeKind::Or`, `NodeKind::Not`
- **And / Or**: ports `a: Bool`, `b: Bool` → `result: Bool`; no config
- **Not**: port `a: Bool` → `result: Bool`; no config
- Default config: `{}` for all three
- `fn run_and`, `fn run_or`, `fn run_not` in `node.rs`
- `ALL` constant grows to 31; update all `match` arms (slug, label, data_inputs,
  data_outputs, default_config, run_summary, the `async_node` / `is_async` list — these
  are pure-sync data nodes, same pattern as `Compare`)

## Phase 3 — Frontend config panel

File: `sentinel-app/src/routes/workflows/+page.svelte`

The `key === "operator"` branch (line 2298) hard-codes numeric ops. Extend it:
- If `selectedNode.kind === "text_compare"` and `key === "operator"`: show
  `["eq", "ne", "contains", "starts_with", "ends_with"]` with friendly labels
- Else (existing Compare): keep `[">", ">=", "<", "<=", "==", "!="]`
- And / Or / Not have no configurable keys — no UI change needed

## Phase 4 — cargo check + deploy

- `task cargo` — confirm clean
- `task cloud-deploy -- sentinel-app sentinel-control sentinel-flow`

## Out of scope (v1)

- Regex match operator (complexity, ReDoS surface)
- Multi-input AND/OR (more than 2 booleans) — chain nodes instead
- Null / empty check node (`IsEmpty`) — can use `TextCompare` with `b = ""` + `eq`
