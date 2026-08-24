# Pure sources run on branches nobody took

**Board card:** #205 Pure sources activate on dead branches — the canvas is telling the truth
**Reported:** a graph with two events and `Get Variable` nodes on both arms lights up every Get
when only one arm executes.

## It is not just the animation

`crates/app-core/src/workflows/engine.rs:499` activates any `is_pure_source` kind
**unconditionally**, without asking whether the branch is live:

```rust
} else if n.kind.is_pure_source() {
    // Pure sources re-evaluate every epoch (no exec propagation) …
    true
}
```

An activated node calls `on_node(nid)`, which is what the editor highlights and what the run log
records. So the canvas is reporting accurately: those nodes really did run, on both arms.

## How bad is it

Mild, but not nothing.

- **No wrong data flows.** The dead arm's consumers still do not run, so nothing downstream acts
  on a value it should not have.
- **Wasted work.** `is_pure_source` is four kinds: `get_variable`, `multi_get_variable`,
  `get_camera_by_name`, `get_recipient`. The last two hit the database. On a graph with several
  of them across several arms, every run pays for every arm.
- **The run log lies by implication.** A reader seeing `get_recipient` in the node list of a run
  reasonably concludes that arm executed.

## Root cause, found (and it is narrower than it looked)

The engine **already** suppresses this — for the Console line only.

`engine.rs:1248-1262` defers a `GetVariable`'s `on_log` into a buffer, and `run_driver` flushes
it at the end of the run only if a node it feeds actually ran:

```rust
if n.kind == NodeKind::GetVariable {
    deferred_get_logs.push((nid, summary, elapsed_ms));   // flushed only if a consumer ran
} else {
    on_log(nid, &summary, elapsed_ms);
}
```

That is #115, and it works: `get_variable_logs_only_on_the_branch_that_ran` asserts exactly the
reported behaviour and passes today.

**But the canvas highlight does not come from `on_log`.** It comes from `on_node`, which
`sentinel-flow/src/engine.rs:273` turns into the `workflow-node` event, which the editor's
per-node highlight controller listens to (`+page.svelte:817`). `on_node` is called at activation
time, unconditionally, for every activated node — pure sources included.

So #115 fixed the log and never reached the animation. Two callbacks, one deferral.

**The fix is to defer `on_node` the same way**: buffer it for pure sources at activation and
emit it under the same "a node it feeds actually ran" condition the logs already use. A Get on a
dead branch then never pings, rather than pinging and being explained away.

Two things to get right while doing it:

- The log deferral is gated on `graph.debug`; the highlight must not be, because the highlight is
  not a debug feature.
- The deferral covers `GetVariable` only — deliberately, per its comment, as the conservative
  case. `GetCameraByName` and `GetRecipient` have the same shape and were left alone. Whether to
  widen it is the same question for both callbacks, and should be answered once.

## The likely fix, and why it needs care

The same scheduler already evaluates a pure source **on demand**, at `engine.rs:559`, when a
consumer gathers its inputs:

```rust
let pure_now = graph.node(edge.from)
    .filter(|u| u.kind.is_pure_source())
    .and_then(|u| node::run_node(u.kind, &u.config, &PortValues::new(), sys) …);
```

That path runs before the `outputs` lookup, so a consumer that runs always gets a fresh value
whether or not the source was activated this epoch. Which means the source is evaluated
**twice** — once as an activated node (the highlight) and once on read (the value) — and the
unconditional activation may be redundant for everything except the case it was added for.

That case is documented and was a real incident (#114/#115): *a pure source with no incoming
edges, feeding a required input, inside a sub-run*. Orphans fall back to `seed_entries`, which is
false for every `OnEvent`/timer sub-path, so without this the source never ran and its consumer's
required input was skipped as a dead branch — silently, with the run reporting `ok`.

So the narrowing to try is: activate a pure source when it is a **true orphan** (no incoming
edges at all), and otherwise let the read path do the work. That preserves the incident fix and
stops lighting up arms nobody took.

**Do not remove the activation.** Measured, not assumed: setting that branch to `false` and
running the suite fails three tests, including the #114/#115 one. The activation is load-bearing.
The bug is in what activation *reports*, not in activation itself — which is why the fix above
touches `on_node` and leaves the condition alone.

**The oracle now exists.** `workflows::golden` has the #114/#115 shape — a pure source with no
incoming edges, feeding a required input, entered as a sub-run — plus its negative case. Any
change here has something to fail against rather than an argument to win.

## Where to look

- `crates/app-core/src/workflows/engine.rs:495-530` — the activation decision.
- `crates/app-core/src/workflows/engine.rs:557-570` — the on-read evaluation.
- `crates/app-core/src/workflows/domain.rs` — `NodeKind::is_pure_source`, and its doc comment,
  which explains the incident this exists for.
- Related: [[ggraph-engine-extraction]] — this same condition is why the engine's `PureSource`
  and this product's `is_pure_source` mean different things, which cost a catalog diff to find.
