# Every edge into a live node lights up, including the ones control never took

**Board card:** #206 Edge highlight follows the target node, not the arm that fired
**Reported:** a graph (BotMessages) where two execution branches converge on one node — both
incoming edges animate, though only one delivered control.

## Cause

`sentinel-app/src/routes/workflows/[[id]]/+page.svelte:3911`:

```svelte
{@const eLive = nodeLive(e.to)}
```

An edge is live when its **target** is live. So every edge entering a highlighted node is
highlighted, whether or not control came that way. With a single incoming edge the answer is
right by accident; with fan-in it is wrong, and fan-in is exactly what a graph with two branches
looks like.

## The editor cannot fix this alone

The `workflow-node` event carries `{ tenant, graph, node, run_id }`. It says a node ran. Nothing
in the stream has ever said **how control reached it**, so there is no local rule the editor
could apply — and the plausible ones are wrong:

- *"highlight only if the source is also live"* — in the reported graph both switches ran, so
  both sources are live. Still two edges.
- *"the most recently lit node delivered it"* — a guess dressed as a heuristic, and wrong the
  moment two nodes light in the same epoch.

## Fix

The scheduler knows. `live_out.insert((nid, arm))` is the exact moment an arm fires, and it is
now in one place (`node::arms_for`, after #204). Report it:

1. `on_arm(node, port)` alongside `on_node` and `on_log` in the driver.
2. `sentinel-flow` publishes `workflow-arm { tenant, graph, node, port, run_id }`.
3. The editor keeps live arms keyed `node:port` and lights an edge when **its own**
   `(from, from_port)` fired.

The node highlight is untouched: it is right already. This is only about the edges.

## Related, and the pattern worth noticing

This is the third bug in the same family and they all have one shape: **the editor inferring
something the engine knew and did not say.**

- #115 — a dead-branch Get's Console line. Fixed by deferring `on_log`.
- #205 — the same Get's canvas highlight, which comes from `on_node` and was never deferred.
- #206 — which edge control travelled along, which nothing reports at all.

Each was found by somebody watching a canvas, and none of them could have been found by reading
the scheduler, because the scheduler is right in all three. What is wrong is what it publishes.
Worth asking, next time a run-visualisation bug appears: *is the engine telling the editor
enough to be correct, or is the editor guessing?*
