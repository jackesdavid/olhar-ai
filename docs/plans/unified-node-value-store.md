# One node-value store — Postgres for values, MinIO for media, no forks

**Board card:** #94 Unified node value store — one place, media in MinIO (ToDo)
**Depends on:** [[workflow-graph-instances]] (#95) — the value context belongs to an *instance*; this
card is where those values live and how media stays out of Postgres.
**Goal:** A node's output values live in **one** place, media lives in object storage as a URL, and
everything that needs a past value reads that same store — the editor, `Show Image`, and a
`DelayTrigger`'s trailing edge alike. Less code than today, not more.

Prompted by [[delay-trigger-recording-signal-flow]] (#86): fixing its trailing edge was about to add
a *fourth* store. The owner stopped it — "everything should be in the same place, do not create
forks of logics" — which is the right call and the reason this card exists.

---

## The forks, as they stand

| Store | Holds | Where |
|---|---|---|
| `workflow_node_state` (Postgres) | `Show Image`'s frame, `Cooldown`'s `last_ms`, `DelayTrigger`'s window | `workflows/repo.rs` |
| `node_frames` (in-memory) | live frames for the editor | `sentinel-flow/src/state.rs:67`, read at `http.rs:160` |
| `media.rs` → **MinIO** | notification snapshot JPEGs | `crates/app-core/src/media.rs` — built, deployed, running |

Three mechanisms for "what did this node produce", and the worst of it: **`engine.rs:871`
base64-encodes a JPEG into a Postgres row** on every `Show Image` run, while `media.rs` — whose own
first line is *"S3-compatible object store (minio) for notification media — snapshot JPEGs"* — sits
right there with its container up. `node_frames` is then a third copy of the same frame, in memory,
lost on restart.

## What forced the issue (#86)

A `DelayTrigger`'s `stop` is delivered by a timer, in a **different run** from the `start`. So the
stop branch can't see what the opening run produced: `StopRecording.camera` comes from `OnEvent` and
its `clip_id` from `StartRecording`, and neither runs on the trailing edge. The engine resolves those
required inputs to nothing and skips the branch as **dead — silently, with the run reporting `ok`**.

The graph is not wrong. Wiring the camera into both `Start` and `Stop`, and `clip_id` from `Start`
into `Stop`, is exactly what the user means. **The engine has to serve that.**

Two attempts were made and both rejected, on the owner's call, correctly:

1. **Camera in/out ports on `DelayTrigger`** (built, reverted in `c3ee151`). It worked, and it made a
   **Control** node know what a camera is. A debounce must not; recording is only its first customer.
2. **A bespoke window snapshot** (`workflows/snapshot.rs`, written, never committed). Closure
   analysis to capture just the values the stop branch reads. Clever, tested — and a fourth store.

The real answer is smaller than either: **if a node's last values are already persisted in one known
place, the trailing edge is not special at all.** It reads them like anything else. `snapshot.rs`
goes away; only its value↔JSON codec is worth keeping.

## The shape

- **`workflow_node_state` is the one place** — a node's last output values, written at run end.
  It already exists and already holds per-node state; this widens its job and deletes the others.
- **Media goes to MinIO** through the existing `media.rs`; the state row holds a **URL**, never
  bytes. Rows go back to being small — which is also what makes "persist every node's outputs"
  affordable, and lets the closure analysis die unbuilt.
- **One reader for everyone**: the editor's live values, `Show Image`'s persisted frame, and the
  `DelayTrigger`'s trailing edge all read the same rows.
- **`node_frames` is deleted** (a cache of the same thing, minus durability), and so is the base64
  path at `engine.rs:871`.

Net: three mechanisms → one, and a Postgres table stops carrying JPEGs.

## Decision (settled by the owner, 2026-07-17) — and the bug it exposed

**The trailing edge reads the values of the run that ARMED its window**, not the last run's. Not a
preference: *"we can have concurrent runs of the workflow, and each concurrent call can create a new
trigger call to start, and each stop should run in the correct context."* Once two contexts are live,
"the last run" is simply the wrong answer.

Pushing on that surfaced something worse. **A window is keyed `(tenant_id, graph_id, node_id)`** —
`workflow_node_state`'s primary key. So there is exactly **one window per Delay Trigger, ever**. Put
two cameras through one graph and:

- Camera A's motion arms the window, starts A's clip.
- Camera B's motion **cannot open its own window** — `try_arm` fails ("already armed"), so B merely
  **extends A's**.
- One `stop` fires, in one context. B's clip is never stopped.

Concurrent contexts don't just lose their values — **they collide**. This makes `(node, run)` the
wrong key too: the *window itself* needs a context, or there can only ever be one. See
[[workflow-graph-instances]] (#95), which this card now depends on.

## Phases

1. **Value codec** — `PortValue` ↔ JSON, tagged, for the kinds worth persisting. Written already in
   the uncommitted `snapshot.rs` (6 tests green); lift it out, drop the closure analysis. Media
   variants (`Image`, `File`) serialize to a **URL** via `media.rs`, not bytes.
2. **Persist at run end** — the engine writes each node's outputs to the store. Decide the key first.
3. **Read it back** — the trailing edge pre-seeds its `outputs` from the store, so `dead` never
   triggers and nothing re-runs. This is the point where #86 closes.
4. **Collapse the forks** — `Show Image` reads/writes the store (frame → MinIO URL); the editor's
   live path reads it; delete `node_frames` and the base64 write.
5. **Retention** — if keyed by run, sweep old rows (the `events` sweep in `0054` is the precedent).

## Verification

- The reported graph, unmodified — camera wired into both `Start` and `Stop`, `clip_id` from `Start`
  into `Stop` — records a clip and **stops it** on the trailing edge. That's the acceptance test, and
  it's the graph the owner drew before any of this.
- Overlapping windows on one camera stop the right clips (whichever key we choose must be shown to
  do this, not assumed).
- No base64 in any Postgres row: `select` for `jpeg_b64` returns nothing.
- A frame survives a pod restart (it's a URL now, not memory).

## Out of scope

- The parameterization epic (input variables, clone, promote-to-input, default templates) — related,
  much bigger, needs its own plan. Its first decision is *how params arrive per trigger*: manual can
  prompt, cron needs config-time defaults, event graphs read the payload.
- Changing `DelayTrigger`'s ports. It has `start` and `stop`, and that's the whole node.
