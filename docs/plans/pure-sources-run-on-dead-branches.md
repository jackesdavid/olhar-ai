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

**Do not fix this blind.** The regression it would reintroduce is silent — a branch skipped as
dead while the run says `ok` — which is precisely the failure mode that took two cards to find
the first time. Add a golden graph covering the #114/#115 shape (pure source, no incoming edges,
required input, sub-run entry) before touching the condition, so the fix has an oracle rather
than an argument.

## Where to look

- `crates/app-core/src/workflows/engine.rs:495-530` — the activation decision.
- `crates/app-core/src/workflows/engine.rs:557-570` — the on-read evaluation.
- `crates/app-core/src/workflows/domain.rs` — `NodeKind::is_pure_source`, and its doc comment,
  which explains the incident this exists for.
- Related: [[ggraph-engine-extraction]] — this same condition is why the engine's `PureSource`
  and this product's `is_pure_source` mean different things, which cost a catalog diff to find.
