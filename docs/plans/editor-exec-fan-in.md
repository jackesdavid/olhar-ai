# Editor: wire multiple exec outputs into one node (fan-in)

**Board card:** #90 Editor: wire multiple exec outputs into one node (fan-in) (ToDo)
**Related:** [[event-storm-backstop-dead]] (#91 — found while investigating this)
**Goal:** Let converging exec paths be wired in the editor. The engine has always supported it; only the
click-to-wire handler blocks it.

## Context — why (and why NOT a Fan In / Fan Out node)

This started as "add two nodes: Fan In and Fan Out". Research killed both.

**Fan Out already works.** `add_edge`'s "already wired" check is keyed only on the *target* side, so one
output pin → many downstream nodes is unrestricted (confirmed empirically: a Text Switch's `arm_2` wired
to two different nodes). A Fan Out node would buy only *naming* — not ordering, not parallelism:
- Execution is single-threaded. `forward_epoch` runs one node at a time; the lone `thread::spawn`
  (`crates/app-core/src/workflows/node.rs:562`) is a watchdog that immediately blocks on `recv_timeout`.
  Real concurrency exists only per *graph run* (`sentinel-flow/src/engine.rs` `spawn_graph_run`), never
  between nodes in one run.
- Branch order is the Kahn tie-break — **node creation order** (position in `graph.nodes` ≈ ascending id),
  not wire or arm order — so branches interleave. `A→B→B2` + `A→C→C2` created in the order A,B,C,B2,C2
  executes A,B,C,B2,C2 (B's chain does *not* finish before C starts). A Fan Out node would not change this.

**Fan In is a real gap — but it's this editor bug.** The engine has always accepted converging exec paths
(`crates/app-core/src/workflows/domain.rs:1890-1901`):
```rust
// exec_in accepts fan-in (multiple execution paths converging on the same node —
// e.g. both branches of an If merging downstream). Data ports stay single-fed.
let is_exec = to_port == EXEC_IN.name;
if !is_exec && self.edges.iter().any(|e| e.to == to && e.to_port == to_port) {
    return Err(format!("input '{to_port}' is already wired"));
}
```
Activation is **OR** (`engine.rs:328-338` breaks on the first live parent) and each node is visited once
per epoch, so converging paths cannot double-fire. But `clickInPort` never reaches `addEdge` when the
input already has a wire — it deletes that wire and returns. Click-to-wire is the only wiring mechanism,
so the capability is unreachable. Hit in practice trying to converge two Text Switch arms on one node.

Fixing the handler unlocks fan-in on **every** node — no new node kind to build or learn.

## Phase 1 — `clickInPort` · `sentinel-app/src/routes/workflows/[[id]]/+page.svelte` (~2187)

Disambiguate *add* vs *remove* with the existing `pending` state. Current code unwires unconditionally:
```ts
const existing = graph.edges.findIndex((e) => e.to === node.id && e.to_port === port);
if (existing >= 0) { await removeEdge(existing); return; }   // never reaches addEdge
```
New shape:
```ts
async function clickInPort(node: Node, port: string) {
  if (!graph || editLocked) return;
  const isExecIn = port === EXEC_IN_NAME;                       // EXEC_IN_NAME = "exec_in" (:821)
  const existing = graph.edges.findIndex((e) => e.to === node.id && e.to_port === port);
  // Clicking a wired input unwires it — except exec_in with a pending output, which
  // adds a converging path (the engine accepts exec fan-in).
  if (existing >= 0 && !(isExecIn && pending)) { await removeEdge(existing); return; }
  if (!pending) { toast.info("Pick an output port first, then this input."); return; }
  // Guard the exact duplicate the backend doesn't reject today (Phase 2).
  const dup = graph.edges.some((e) => e.to === node.id && e.to_port === port
    && e.from === pending!.node && e.from_port === pending!.port);
  if (dup) { pending = null; toast.info("Those pins are already wired."); return; }
  /* …rest unchanged: optimistic push + data.addEdge + revert on error… */
}
```
Keep the **data-port path byte-identical** — data inputs stay single-fed and keep unwire-on-click.

## Phase 2 — Backend duplicate guard · `crates/app-core/src/workflows/domain.rs` `add_edge`

The doc comment promises `add_edge` rejects "an exact duplicate", but **no such check exists**. For data
ports the "already wired" check hides it; for `exec_in`, `is_exec` bypasses that check entirely. It's
unreachable today only because the UI blocks second wires — Phase 1 exposes it. Add before the push:
```rust
if self.edges.iter().any(|e| e.from == from && e.from_port == from_port
    && e.to == to && e.to_port == to_port) {
    return Err("those pins are already wired".into());
}
```
This makes the doc comment true. Add a unit test beside
`add_edge_allows_exec_back_edge_but_rejects_data_cycle`.

## Removal UX — already solved, no work

Wires already support **Alt+click / "break" mode** to delete a specific wire
(`+page.svelte:2932`: `if (editorMode === "break" || ev.altKey) { removeEdge(idx); return; }`).
Clicking a multi-wired `exec_in` pin with nothing pending removes the *first-added* wire (`findIndex`);
Alt+click the wire itself for precision.

## Check, don't assume

`unlinkPort` (`+page.svelte:2176`) also uses `findIndex` (first wire only), but its only caller is the
inspector's unlink button (`:3658`) on **data** rows — confirm it's unreachable for `exec_in`, then leave it.

## Verification

- Wire a Text Switch's `arm_1` **and** `arm_2` into the same downstream node's `exec_in` → both wires
  persist and render; reload the graph → both survive (the backend accepted them).
- Run it: fire `arm_1` → downstream runs **once**; fire `arm_2` → runs **once**. No double-fire.
- Wire the same arm into the same pin twice → rejected with a toast, no duplicate wire (Phase 2).
- Data input still single-fed: a second data source behaves exactly as today.
- Alt+click one converging wire → only that wire is removed; the other survives.

## Out of scope

- A Fan In node kind (the fix makes it unnecessary).
- **AND/join** semantics (wait for *all* inputs before firing once) — genuinely new engine work: the
  activation check would count live parents instead of breaking on the first, and only reliably sees
  branches that fire in the same epoch. Not asked for.
- Named/multiple exec **input** pins — no mechanism exists; `exec_in` is a single hardcoded constant
  (`domain.rs:820`) checked before config-derived ports, and the editor draws exactly one exec-in circle.
- Data-port fan-in (two values racing into one input has no sensible answer; the backend rightly rejects).
- Fan Out node; branch ordering; intra-run parallelism.
