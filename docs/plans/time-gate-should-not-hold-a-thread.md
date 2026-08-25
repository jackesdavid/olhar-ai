# Time Gate holds a thread open for its whole window

**Board card:** #207 Time Gate keeps a keeper thread; Delay Trigger already knows better
**Found while:** converting the scheduler's inline kinds to engine behaviours (#204).

## What it does now

`engine.rs`, the `TimeGate` block: on the first signal it writes `state: armed` with an
`expires_at`, registers itself in an in-memory `active_gates` set, and then **spawns a thread
that polls the database every 250 ms** until either the state stops being armed or the deadline
passes. The `out` arm fires from inside that loop.

So a ten-minute gate holds a thread, a database connection's worth of polling, and the run
itself, for ten minutes.

## What the node beside it already does

`DelayTrigger` has the same shape — a window that opens on a signal, extends on further signals,
and fires on the trailing edge — and it has no thread:

- `repo::try_arm_node_state(.., expires_at)` — compare-and-set, so two pods cannot both open it.
- `crate::work_queue::enqueue_window_stop_blocking(.., expires_at)` — a `work_jobs` row with
  `run_after` set to the deadline.
- The trailing edge arrives as a **new run**, entered at that node, recognised by
  `WINDOW_STOP_MARKER` in the entry payload.

Zero threads, zero polling, and it survives a pod restart because the deadline is a row rather
than a live stack frame.

**This is not a limitation of the design. It is debt.** Two nodes with one shape, solved two ways,
and the older way is the one that holds a thread.

## The fix, and the one thing it changes

Make `TimeGate` follow `DelayTrigger`: arm, enqueue, extend by rewriting `run_after`, fire on
re-entry.

The behaviour that moves is real and worth stating rather than discovering: **`out` currently
fires inside the SAME run; afterwards it fires in a NEW one.** Three consequences, all of which
`DelayTrigger` already lives with, so none are hypothetical:

- Run history shows two runs instead of one long one.
- Anything downstream of the gate stops seeing run-scoped VARIABLES set before it.
- The trailing run needs the marker in its entry payload.

## The trap, already documented next door

`DelayTrigger`'s comment records it and it would have been repeated as a bug: the test for "the
window closed" is the **marker in the payload**, not `forced` alone. A loop back-edge forces a
node too, and mistaking one for the other makes a loop close the window.

## Verification

The differential test (`workflows/differential.rs`) compares this product's scheduler with the
engine's, node for node. A `TimeGate` shape belongs in it once this lands — and the conversion is
what makes the shape expressible at all, since a keeper thread is not something `NodeStep` can
describe and `schedule()` + re-entry is.
