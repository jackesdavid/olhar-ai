# Bug — Get Variable nodes activate/log on runs that don't use them

**Board card:** #115 Get Variable over-activation — fires on branches that don't use it (Bugs)
**Related:** [[stop-recording-missing-camera]] (#114 — the fix whose sub-run correctness must NOT
regress), [[workflow-graph-instances]] (#95)

## Symptom (real, "Recording By Movement" — after #114 landed)

#114 is confirmed fixed: the trailing edge now stops the clip correctly. But the run logs (and the
canvas) show **every `Get Variable` firing on every run**, including Gets that feed the *other*
branch:

- On the **stop** run (trailing edge): `Get $current_camera`, `Get $current_clip_id`, **`Get
  $pre_roll_seconds`** and a second `Get $current_camera` all log — yet `pre_roll_seconds` +
  `current_camera` there only feed **Start Recording**, which does not run on a stop.
- On the **start** run: the stop branch's `Get current_camera` / `Get current_clip_id` (which feed
  Stop Recording) also light up.

Cosmetic — the values are correct, nothing misbehaves — but it's misleading in the log and on the
canvas (the start-branch Gets visibly highlight during a stop). Owner: "is minor … this is visual."

## Root cause (confirmed in code)

`GetVariable` is a `NodeKind::is_pure_source()` (`domain.rs`). In `forward_epoch`
(`crates/app-core/src/workflows/engine.rs` ~L462) a pure source activates **unconditionally every
epoch/run** — deliberately, and the comment says why: a pure source with no incoming edges is a true
orphan, and orphans fall back to `seed_entries`, which is **false on every sub-run**, so without this
exemption the source silently wouldn't run and its consumers' required inputs would be skipped as a
dead branch (exactly the #114 class of bug). So the exemption is load-bearing — **do not remove it.**

The side effect: a pure `GetVariable` runs (and fires the `on_log(nid, summary, …)` callback that
drives both the run-log line and the live canvas activation) even when the only nodes it feeds are on
an unreached exec branch.

## Fix direction (proportionate to "minor" — do NOT change activation)

Keep pure-source activation as-is (sub-run correctness depends on it). Instead, **suppress the
per-node log / UI activation for a pure `GetVariable` whose produced value is not consumed by any
node that ran this run.** i.e. only emit its `on_log` when at least one node it feeds (a data edge
`from == getVar`) is in `ran`. A Get with a consumer that ran → shows; a Get whose consumers all sat
out (the other branch) → stays quiet. This fixes both the log and the canvas highlight (same emit),
touches no activation/value semantics, and can't regress #114.

- Impl seam: the `on_log` call site(s) for the pure/Get path in `run_driver`/`forward_epoch`, or the
  log-buffering in `sentinel-flow/src/engine.rs` (`node_logs`), whichever has `ran` + the graph edges
  in hand. A Get feeding ≥1 ran node emits; else it's dropped from the run log + UI stream.
- Edge: a Get with **no** consumers at all (a truly unused variable read) → also quiet (nothing used
  it). A Get feeding several consumers, any of which ran → emit.

## Verification

- "Recording By Movement": the **stop** run logs only the stop-branch Gets (`current_camera`,
  `current_clip_id`); `pre_roll_seconds` does not appear. The **start** run logs only the start-branch
  Gets. The canvas highlights only the Gets whose branch ran.
- #114 still holds: the trailing edge still resolves `camera`/`clip_id` and stops the clip (activation
  unchanged; only the log/UI emit is gated).
- A normal single-branch graph is unaffected (its Gets all feed ran nodes → all still shown).

## Out of scope

- Demand-driven (lazy) pure-source evaluation — a bigger engine change; not needed for a cosmetic fix.
- Changing `is_pure_source` / activation semantics.
