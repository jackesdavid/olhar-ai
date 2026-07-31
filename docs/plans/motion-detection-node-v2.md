# Motion Detection Node v2 — Implementation Plan

**Board card:** Motion Detection Node v2 (ToDo)
**Depends on:** [[event-payload-data]] (the v2 node emits an event carrying the motion result)
**Goal:** A self-contained motion node that **picks its own camera**, watches that camera's stream, and **emits an event only when motion is detected** — with a configurable **minimum interval between emissions** to throttle signal throughput.

## Shipped (v1) — camera-bound `MotionWatch`

Implemented the camera-bound detector (the headline ask) on existing rails, **no engine/execution-model change**:
- New `NodeKind::MotionWatch` (`crates/app-core/src/workflows/domain.rs`) — Vision category. Config: `camera`, `sensitivity`, `pixel_threshold`, `sample_gap_ms`, `timeout_secs`. Outputs `moved` (Bool) + `score` (Num).
- `run_motion_watch` (`node.rs`): resolves its own camera, grabs **two frames `sample_gap_ms` apart**, diffs via the shared `motion_score` helper (refactored out of v1's `run_motion_detection`). IO-bound (20s watchdog).
- Frontend auto-derives from `NodeKind::ALL`; the `camera` config reuses the existing device picker — zero FE changes.
- **Compose to emit:** wire `moved` → `If` → `Emit event` (now payload-capable via [[event-payload-data]], e.g. carry `score`). Put the graph on a **Cron** trigger to watch continuously; the cron interval sets the cadence.

**Deferred (needs a decision + plumbing):** the *built-in* emit + the **min-interval throttle** the use case asks for. Both need **per-node cross-run state** (last-emit timestamp), which the node can't reach today — `run_node` gets only `ToolContext` (no node/graph id, no persistent store). That's the Route A vs B fork below; v1 ships the detection half and composes the rest. True always-on streaming (Route A) is the eventual upgrade.

## What's wrong with v1 (original MotionDetection)

`MotionDetection` (`crates/app-core/src/workflows/domain.rs:105`, exec `node.rs:543-573`) is a pure function: it takes **two `image` inputs** (`image_a`, `image_b`), resizes to 320×240 grayscale, counts pixels whose luma differs by > `pixel_threshold`, and outputs `moved`/`score`/`ok`. The graph author must wire two `GetCurrentFrame` nodes, time them, and loop — it doesn't own a camera, doesn't watch continuously, and doesn't emit. The user wants a **camera-bound, continuously-watching emitter**.

## The central architecture question — how does "continuous" run?

Today execution is **one-shot**: a graph is triggered (Manual/Cron), runs its DAG to completion (≤ `WF_MAX_RUNTIME` 60 s, ≤ `WF_MAX_STEPS` 10k — `sentinel-flow .../engine.rs:20-22`), and stops. **There is no always-on / streaming node** (confirmed: no continuous pattern in the workflow module). So "read the stream continuously and emit" doesn't fit the model as-is. Two routes — **this is the decision to make before building** (see Open decisions):

- **Route B — Cron-sampled stateful node (pragmatic, ships on existing rails).** The graph is `Cron { every_secs }`-triggered; the motion node grabs the **current** frame, compares to the **previous frame it persisted** (node keeps last-frame state across runs, keyed by `(tenant, graph, node)` — same shape as the new `node_frames` cache), emits when moved + throttle satisfied. "Continuous" becomes "sampled at the cron rate." No new execution model.

Recommendation: **B for v1** (delivers the camera-bound emitter + throttle on current infra), with A noted as the eventual "true continuous" upgrade.

## Phase 1 — Node shape + camera binding (assuming Route B)
- New config-rich `MotionDetection` v2 (extend the existing kind, or add `MotionWatch` and deprecate v1 — see Open decisions). Config:
  - `camera` — device selection (reuse `device_actions::resolve_device`, like `GetCurrentFrame`/`CameraSnapshot` `node.rs:839-878`).
  - `sensitivity`, `pixel_threshold` — carried from v1.
  - `min_interval_secs` — minimum time between emitted events (throttle).
  - `event` — the event to emit on motion (ties to [[event-payload-data]]).
- It is an **emitter** like `EmitEvent`: exec passthrough + fires the configured event. No `image_a`/`image_b` inputs — it owns frame-grabbing internally via `sys.frames.current(device, max_age)`.

## Phase 2 — Stateful frame diff + throttle
- Persist per-node state across runs: last frame bytes + last-emit timestamp, keyed `(tenant, graph, node)` in `AppState` (mirror the `node_frames` map added in [[show-image-node-rendering]]).
- On each run: grab current frame → if a previous frame exists, run the v1 diff → if `score >= sensitivity` **and** `now - last_emit >= min_interval_secs`, emit the event and update `last_emit`. Always update `last_frame`.

## Phase 3 — Emit with payload
- Via [[event-payload-data]], emit the configured event carrying a payload — e.g. `score` (Num) or the triggering frame (Image) — so the `OnEvent` handler (same or other graph) can act on it.
- Decide the default payload field (see Open decisions).

## Phase 4 — Frontend
- Catalog auto-derives from `NodeKind::ALL` (no manual catalog edit). Config editor: camera picker (reuse the device combobox), sensitivity/threshold sliders, `min_interval_secs`, and the event picker (shared with EmitEvent).
- House style: helpers for any markup logic; web-only shadcn.

## Open decisions (grill before building)
1. **Route A vs. Route B** — true always-on watcher subsystem vs. cron-sampled stateful node. Recommend B for v1. (If A: where does the watcher lifecycle live, and how is it started/stopped per graph enable/disable?)
2. **Extend `MotionDetection` or add a new kind?** v1 is a pure 2-image function some graphs may use. Adding `MotionWatch` keeps v1 intact; extending overloads one kind with two very different shapes. Recommend a **new kind**.
3. **Payload of the emitted event** — `score` (Num), the frame (Image), or both? (Needs [[event-payload-data]]; v1 currently allows one field.)
4. **Sampling rate (Route B)** — driven purely by the graph's `Cron every_secs`, or a node-local `sample_interval` independent of the trigger? Cron is simplest; a node-local rate is more self-contained.
5. **Region of interest / zones** — limit motion to a sub-rectangle of the frame? Out of scope for v1 unless wanted.

## Out of scope (v1)
- True always-on streaming (Route A) if v1 ships as Route B.
- Per-zone / ROI masking, object-aware motion (that's the vision detector's job).
- Cross-pod watcher coordination ([[multi-pod-flow-event-fabric]]).
