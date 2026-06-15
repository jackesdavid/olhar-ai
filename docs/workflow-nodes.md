# Sentinel Workflow Nodes — reference

A workflow **graph** is a trigger + a set of typed **nodes** wired by **edges**. This is the
canonical list of every node, its ports, config, and behavior, plus the cross-cutting
concepts (exec flow, memoize, loops, events, triggers).

> Source of truth: `app/crates/core/src/workflows/domain.rs` (node catalog + ports) and
> `node.rs` (behavior). The editor palette is generated from the catalog, so this doc
> should be updated whenever a `NodeKind` is added/changed.

---

## Core concepts

### Exec flow vs data
Every node has two kinds of ports:
- **Exec ports** (white): `exec_in` / `exec_out` — *control flow*. They carry no value;
  they sequence and gate execution. `If` replaces `exec_out` with `true` / `false`.
- **Data ports** (typed: text, num, bool, image, recipient, dictionary, file) — values
  flowing along wires. A `*` marks a **required** input.

A node runs when it's reached on the exec chain (or it's an entry node with no incoming
exec edge) **and** its required inputs resolve. Data inputs come from a wired upstream
output, else from the node's config literal. A required input wired to a node that didn't
run (e.g. a dead `If` branch) puts this node on a dead path — it's skipped.

### Memoize (per node, default ON)
Each node has a **Memoize** toggle (inspector). When **on** (default), the node's output
is computed once per run and reused on fan-out. When **off**, the node **recomputes every
time it's reached** — required for **loop bodies** and **live sources** like
`Get current frame` (otherwise a loop runs once / a frame is stale).

### Loops
You may wire an **exec back-edge** (an `exec_out` back to an upstream `exec_in`) to form a
loop. (Data back-edges are rejected — they'd have no defined value.) Loop-body nodes must
be **memoize = off** to iterate. Every run is bounded by a **step budget (10,000 node
runs)** and a **wall-clock budget (60 s)**; exceeding either fails the run with an error.

### Events (in-graph, async) — see `EmitEvent` / `OnEvent` below
There are **no built-in events** — you define them by name. `EmitEvent("x")` fires the
`OnEvent("x")` handler subtree as a **decoupled, concurrent sub-run**, **dropped while that
handler is still running** (coalesce-drop). Scope is the **same graph** (cross-graph is not
available yet).

### Triggers (graph-level, not nodes)
How a whole graph starts a run (set in the graph header, not a node):
- **Manual** — only via Run-now.
- **Cron { every_secs }** — a fresh run every N seconds.
- **OnNewAlert { min_level }** — a run per new alert at/above a level.

### Concurrency policy (graph-level)
What happens when a graph fires while a previous run is in flight:
`SkipIfRunning` · `QueueSingle { max_queued }` · `Concurrent { max_concurrent }`.

---

## Nodes by category

Legend: **In** = data input ports, **Out** = data output ports (every node also has
`exec_in`/`exec_out` unless noted). `*` = required.

### Cameras

#### Camera snapshot
- **In:** `camera*` (text) · **Out:** `image`, `result`, `ok`
- **Config:** `camera` (name/ip/"first")
- **Behavior:** Grabs a still **on demand** — fast from the warm go2rtc feed, falling back
  to ONVIF/ffmpeg if the stream is cold. Best freshness/quality; may connect a cold camera.
- **Use for:** one-off captures (a snapshot in an alert, a manual grab).

#### Get current frame
- **In:** `camera*` (text) · **Out:** `image`, `age_ms` (num), `ok`
- **Config:** `camera`, `max_age_ms` (default 2000)
- **Behavior:** Returns the **most recent frame from a warm in-memory grabber** (~zero
  latency, no per-call round trip). Starts the grabber on first use; falls back to a
  snapshot until warm. `age_ms` is how old the returned frame is.
- **Use for:** high-rate loops (e.g. recognize every second). **Set memoize = off** so each
  pass/tick gets a fresh frame.

### Vision / AI

#### Person detection
- **In:** `image*` · **Out:** `person` (bool), `count` (num), `confidence` (num %), `ok`
- **Behavior:** Detects whether a person is present (runs **locally** via the on-device
  detector, no sidecar).

#### Recognize person
- **In:** `image*` · **Out:** `person` (name), `person_id`, `confidence` (num %), `ok`
- **Config:** `max_distance` (default 0.4 — cosine threshold for a match)
- **Behavior:** Embeds the largest face (AI sidecar) and runs a kNN over the enrolled
  people (pgvector). `ok` is true when a match within `max_distance` is found.

#### Describe image
- **In:** `image*` · **Out:** `description` (text), `ok`
- **Config:** `label` (what it is, for context)
- **Behavior:** A natural-language caption of the frame via the vision model.

#### Motion detection
- **In:** `image_a*`, `image_b*` · **Out:** `moved` (bool), `score` (num %), `ok`
- **Config:** `sensitivity`, `pixel_threshold`
- **Behavior:** Compares two frames and reports whether/how much they differ.

### Alerts

#### Get last alert
- **Out:** `id`, `level`, `message`, `room`, `source`, `created_at`, `ok` · (no inputs)
- **Behavior:** Source node — reads the most-recent alert into typed outputs.

#### Create alert
- **In:** `message*`, `room` · **Out:** `id`, `result`, `ok`
- **Config:** `message`, `room`, `level` (info/warn/…), `source`
- **Behavior:** Writes a new alert.

#### Deliver alert
- **In:** `id*` · **Out:** `result`, `ok`
- **Behavior:** Delivers an existing alert (by id) to its recipients.

### Recipients / notifications

#### Get recipient
- **Out:** `recipient`, `ok` · (no inputs) · **Config:** `recipient`
- **Behavior:** Source node — picks a notification recipient as a typed `recipient` value.

#### Notify recipient
- **In:** `message*`, `recipient`, `image` · **Out:** `result`, `ok`
- **Config:** `message`
- **Behavior:** Sends a notification (Telegram). An unwired `recipient` auto-targets the
  sole enabled recipient; an optional `image` attaches a frame.

### Text / network

#### Format
- **In:** `p1`, `p2`, `p3` (text) · **Out:** `text`
- **Config:** `template` (e.g. `"{p1} — {p2}"`), `p1`–`p3`
- **Behavior:** Substitutes `{p1..p3}` into the template. Pure text composition.

#### HTTP post
- **In:** `url*`, `headers` (dict), `fields` (dict), `file` · **Out:** `status` (num),
  `response` (text), `ok`
- **Behavior:** POSTs form data (multipart when a `file` is wired) to a URL.

### Flow control

#### If
- **In:** `condition*` (bool) · **Exec out:** `true` / `false` (instead of `exec_out`)
- **Behavior:** Routes the run down the `true` or `false` exec branch. The other branch
  (and everything downstream of it) is gated off.

#### For each
- **In:** `items` (text) · **Out:** `item` (text), `index` (num) · **Exec out:** `loop_body`
  / `completed` (instead of `exec_out`)
- **Config:** `items` — the list, as a comma-separated string (`alice, bob`) or a JSON array
  (`["a","b"]` / `[1,2,3]`). May also be wired from a text output.
- **Behavior:** Iterates the list **synchronously**. Fires `loop_body` once per element —
  exposing the current `item` and 0-based `index` — and runs that branch to completion
  before the next element. After the last element, fires `completed` exactly once. Empty
  list → just `completed`. Type-agnostic: elements arrive as text (numbers as their text
  form, e.g. `"42"`).
- **Use for:** processing a list (recipients, rooms, detected names…). **Body nodes must be
  memoize = off** so they re-run each iteration. Bounded by the run's step/time budget
  (very large lists will hit it).

#### Wait
- No data ports (exec-only) · **Config:** `seconds`
- **Behavior:** Delays the run by `seconds`, staying active until it elapses.

#### Emit event
- No data ports (exec-only; `exec_in` + pass-through `exec_out`) · **Config:** `event`
- **Behavior:** Fires the named event → (re)runs the matching `On event` subtree as a
  **decoupled, concurrent sub-run**. **Dropped while that handler is already running**
  (coalesce). Exec passes through so the main flow continues immediately. Carries no data.

#### On event
- **Exec out only** (entry node) · **Config:** `event`
- **Behavior:** Entry point for a named event. **Never runs on the graph's normal
  trigger** — only when a matching `Emit event` fires. Its `exec_out` drives the handler.

#### Print
- **In:** `message`, `on_screen` (bool) · **Config:** `message`, `on_screen`, `level`
- **Behavior:** Prints to the editor console (always) and, unless `on_screen` is off,
  flashes the text over the canvas. Debug sink.

#### Show image
- **In:** `image*` · **Config:** `label`
- **Behavior:** Renders the input frame inside the node on the canvas. Debug sink for
  eyeballing a frame; emits no data.

---

## How to use events (`Emit event` / `On event`)

Events let a fast path trigger a slower, decoupled handler without blocking it or piling up.

1. **Define the handler.** Add an **On event** node, set its `event` to a name, e.g.
   `person_seen`. Wire its `exec_out` into a **self-contained** subtree — it must
   re-acquire its own data (events carry no payload in v1):
   `On event("person_seen") → Get current frame → Recognize person → Notify recipient`.
2. **Fire it.** On your main flow, add an **Emit event** node with the **same** `event`
   name on the exec chain, e.g. after an `If`:
   `Cron 1s → Get current frame (memoize off) → Person detection → If(person) →(true)
   Emit event("person_seen")`.
3. **Behavior at runtime.** Each second the loop emits `person_seen`; the handler runs
   concurrently. If a previous `person_seen` handler is still running, the new fire is
   **dropped** — so detections coalesce instead of queueing.

**Notes & limits (v1):**
- Events are **in-graph only** (same graph). No cross-graph bus yet.
- The "skip if still running" guard is keyed per `(graph, event-name)`.
- Handler subtrees should be self-contained; data wired from outside the handler won't be
  available in the sub-run.
- `On event` is inert on `Manual`/`Cron`/`OnNewAlert` triggers — it only fires on its event.

---

## Worked example: recognize anyone in a room ≥ 1×/second

```
Trigger: Cron { every_secs: 1 }, Concurrency: SkipIfRunning

[Get current frame]            memoize = OFF   (camera = "Front")
      │ image
      ▼
[Person detection]
      │ person (bool)
      ▼
[If] ──true──▶ [Emit event "person_seen"]
      └─false─▶ (idle)

[On event "person_seen"]
      │ exec
      ▼
[Get current frame]  memoize = OFF
      │ image
      ▼
[Recognize person]  (max_distance 0.4)
      │ person, ok
      ▼
[If ok] ──true──▶ [Notify recipient]  message: "Seen {person}"
```

The 1 Hz loop stays cheap (warm frame + local person detection) and only kicks off the
heavier recognize-and-notify handler when a person is present — and never more than one
handler at a time.

---

## Worked example: notify a list of rooms

```
[For each]  items = "lobby, garage, roof"
  ├─ loop_body ─▶ [Format] "Sweep {p1}"  (p1 = item, memoize OFF) ─▶ [Notify recipient]
  └─ completed ─▶ [Print] "sweep done"
```

`loop_body` fires 3× (item = lobby, garage, roof; index = 0,1,2), each running the
Format→Notify branch to completion; then `completed` fires once. The Format/Notify nodes
must be **memoize = off** to re-run per item.

---

## Adding a new node (for maintainers)

1. `domain.rs`: add the `NodeKind` variant + add it to `NodeKind::ALL`, and the
   `slug` / `label` / `category` / `input_ports` / `output_ports` / `default_config` arms
   (+ a port-table const if it has data ports).
2. `node.rs`: add a `run_node` match arm (and a `debug_summary` arm).
3. The editor palette + inspector pick it up automatically from the catalog. No TS change.
4. Engine-level special behavior (like `If` / `ShowImage` / `Emit event`) lives in
   `engine.rs` (`forward_epoch`), not in `run_node`.
