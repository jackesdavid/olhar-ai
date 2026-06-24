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

### Typed arrays & detection elements
Beyond the scalar port types there are **typed arrays** — `String[]`, `Number[]`,
`PersonDetection[]`, `FaceDetection[]` — and the struct **elements** `PersonDetection` /
`FaceDetection`. Producer nodes emit arrays (e.g. Person detection → `persons:
PersonDetection[]`); **For each** iterates any of them and yields the element on `item`;
**Break** nodes destructure an element into its fields. `For each`'s `items`/`item` use the
**`Any`** wildcard so one loop handles every type — which means an element wired into a
type-specific port isn't checked at edit time (a mismatch just reads as empty at run time).

### Memoize (per node, default ON)
Each node has a **Memoize** toggle (inspector). When **on** (default), the node's output
is computed once per run and reused on fan-out. When **off**, the node **recomputes every
time it's reached** — required for **loop bodies** and **live sources** like
`Get current frame` (otherwise a loop runs once / a frame is stale).

### Per-node timeout (IO nodes)
Nodes that reach the network/DB (cameras, vision/AI, recognize, HTTP, notify/deliver, create
alert) run under a **per-node deadline** so a hung call can't stall a run forever — exceeding it
fails the run with `"<Node> timed out after Ns"` (freeing its concurrency slot). Sensible
per-kind defaults apply (snapshot/get-frame 10 s, detection/recognize 30 s, HTTP/notify/deliver
35 s, describe 65 s, create alert 15 s); override per node with a `timeout_secs` config value
(`0` disables it). Pure/instant nodes and `Wait` (a deliberate sleep) are never watched. This is
the per-node complement to the run-wide 60 s budget below.

### Loops
You may wire an **exec back-edge** (an `exec_out` back to an upstream `exec_in`) to form a
loop, or use the **For each** node for list iteration. (Data back-edges are rejected — they'd
have no defined value.) Loop-body nodes must be **memoize = off** to iterate. Every run is
bounded by a **step budget (10,000 node runs)** and a **wall-clock budget (60 s)**; exceeding
either fails the run with an error.

### Events (in-graph, async) — see `Emit event` / `On event` below
There are **no built-in events** — you define them by name. `Emit event("x")` fires the
`On event("x")` handler subtree as a **decoupled, concurrent sub-run**, **dropped while that
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

## Nodes

Legend: every node also has `exec_in` / `exec_out` (control flow) **unless** the Outputs
column lists explicit exec pins. `*` = required input. All nodes have a **Memoize** toggle
(default on; turn off for loop bodies / live sources).

### Cameras

| Node | Inputs | Outputs | Config | Behavior | Sample usage |
|---|---|---|---|---|---|
| **Camera snapshot** | `camera*` | `image`, `result`, `ok` | `camera` (name/ip/"first") | On-demand still from the warm go2rtc feed (ONVIF/ffmpeg fallback). Best quality; may wake a cold camera. | `Camera snapshot(camera="Front") → Describe image` |
| **Get current frame** | `camera*` | `image`, `age_ms`, `ok` | `camera`, `max_age_ms` (2000) | Most-recent frame from a warm in-memory grabber (~0 latency); falls back to a snapshot until warm. | `Cron 1s → Get current frame (memoize off) → Person detection` |

### Vision / AI

| Node | Inputs | Outputs | Config | Behavior | Sample usage |
|---|---|---|---|---|---|
| **Person detection** | `image*` | `count`, `confidence` %, `persons` (**PersonDetection[]**), `ok` | — | Detects people locally (on-device, no sidecar). `persons` is one detection (box + confidence) per person — iterate it (a single person = a loop of 1). | `Get current frame → Person detection → (persons) → For each` |
| **Detect objects** | `image*`, `class` (Text) | `count`, `confidence` %, `detections` (**PersonDetection[]**), `ok` | `class` (any COCO class name, default `person`) | Detects any of the 80 COCO classes locally. Choose the class in the property panel dropdown or wire `class` to override at runtime. Outputs the same shape as Person detection — plug into `For each` + `Break detection`. | `Get current frame → Detect objects(class=car) → For each → Break detection → bbox` |
| **Break person detection** | `person` (Person) | `bbox` (Number[]), `confidence` % | — | Destructures a `PersonDetection` element into its fields (Unreal-style "Break"). | `For each(persons) → item → Break person detection → bbox / confidence` |
| **Break detection** | `detection` (Person) | `bbox` (Number[]), `confidence` % | — | Destructures a generic `Detection` element (from Detect objects) into its fields. Same as Break person detection but paired with the Detect objects node. | `For each(detections) → item → Break detection → bbox / confidence` |
| **Break face detection** | `face` (Face) | `bbox` (Number[]), `confidence` %, `landmarks` (count) | — | Destructures a `FaceDetection` element into its fields. | `For each(faces) → item → Break face detection` |
| **Recognize person** | `image*` | `person`, `person_id`, `confidence` %, `ok` | `max_distance` (0.4) | Embeds the largest face (AI sidecar) + kNN over enrolled people (pgvector). `ok` = match within threshold. | `Camera snapshot → Recognize person → If(ok) → Notify recipient` |
| **Describe image** | `image*` | `description`, `ok` | `label` | Natural-language caption of the frame via the vision model. | `Camera snapshot → Describe image → Notify(message=description)` |
| **Motion detection** | `image_a*`, `image_b*` | `moved` (bool), `score` %, `ok` | `sensitivity`, `pixel_threshold` | Compares two frames; reports whether/how much they differ. | `snapshot A + snapshot B → Motion detection → If(moved)` |
| **Draw BBox** | `image*` (Image), `bbox*` (Number[]) | `image` (Image), `ok` | `color` (#RRGGBB, default `#00FF00`), `thickness` (px, default `2`), `format` (`xyxy`/`xywh`, default `xyxy`) | Draws a bounding box rectangle on the image at the given coords → annotated JPEG. Wire `format=xywh` when coming from Motion detection; `format=xyxy` (default) when coming from Break person/face detection. | `Person detection → For each → Break person detection → (bbox) Draw BBox → Show image` |
| **Blur Outside Box** | `image*` (Image), `bbox*` (Number[]) | `image` (Image), `ok` | `blur` (Gaussian σ, default `12`), `format` (`xyxy`/`xywh`, default `xyxy`) | Blurs everything *outside* the bounding box — the region inside stays sharp → redacted JPEG. Privacy-preserving framing (e.g. blur the scene, keep only the person). Same `format` rule as Draw BBox. | `Person detection → For each → Break person detection → (bbox) Blur Outside Box → Show image` |
| **AI If** | `image` (Image, optional), `question` (Text, optional) | exec: `true`, `false` | `question` (string), `timeout_secs` (default `65`) | Asks the vision model a yes/no question about an optional image. Routes exec to `true` or `false` exactly like `If` but powered by Claude. `question` can be wired (Text) or set as a config literal; no image wired = text-only question (e.g. "Is it after 18:00?"). Fails the run if no question is provided. | `Camera snapshot → AI If(question="Is there smoke?") →true Create alert` |

### Alerts

| Node | Inputs | Outputs | Config | Behavior | Sample usage |
|---|---|---|---|---|---|
| **Get last alert** | — | `id`, `level`, `message`, `room`, `source`, `created_at`, `ok` | — | Source node: reads the most-recent alert into typed outputs. | `Get last alert → Format("[{p1}] {p2}", p1=level, p2=message)` |
| **Create alert** | `message*`, `room` | `id`, `result`, `ok` | `message`, `room`, `level`, `source` | Writes a new alert. | `If(motion) → Create alert(message="Motion", level="warn")` |
| **Deliver alert** | `id*` | `result`, `ok` | — | Delivers an existing alert (by id) to its recipients. | `Get last alert → Deliver alert(id)` |

### Recipients / notifications

| Node | Inputs | Outputs | Config | Behavior | Sample usage |
|---|---|---|---|---|---|
| **Get recipient** | — | `recipient`, `ok` | `recipient` | Source node: picks a notification recipient as a typed value. | `Get recipient → Notify recipient(recipient)` |
| **Notify recipient** | `message*`, `recipient`, `image` | `result`, `ok` | `message` | Sends a Telegram notification. Unwired `recipient` = the sole enabled one; optional `image` attaches a frame. | `Recognize person → Notify recipient(message="Seen {person}", image)` |

### Text / network

| Node | Inputs | Outputs | Config | Behavior | Sample usage |
|---|---|---|---|---|---|
| **Format** | `p1`, `p2`, `p3` | `text` | `template`, `p1`–`p3` | Substitutes `{p1..p3}` into the template. Pure text composition. | `Get last alert → Format("Alert: {p1}", p1=message)` |
| **HTTP post** | `url*`, `headers`, `fields`, `file` | `status`, `response`, `ok` | `url`, `headers`, `fields`, `file` | POSTs form data (multipart when a `file` is wired) to a URL. | `Camera snapshot → HTTP post(url=…, file=image)` |

### Flow control

| Node | Inputs | Outputs | Config | Behavior | Sample usage |
|---|---|---|---|---|---|
| **If** | `condition*` (bool) | exec: `true` / `false` | — | Routes the run down the true/false exec branch; the other branch is gated off. | `Recognize person → If(ok) →true Notify recipient` |
| **String switch** | `value*` (Text) | exec: `<arm_1>`, `<arm_2>`, … , `default` | `arms` (list of labels, default `["arm_1","arm_2"]`) | Routes exec to the arm whose label exactly equals the incoming string (case-sensitive). If no arm matches, the built-in `default` arm fires. Arms are added/removed in the inspector — each becomes a named exec output port. | `Describe image → (description) → String switch (arms: red, blue) →red Notify recipient` |
| **For each** | `items` (**any array** — `String[]`/`Number[]`/`PersonDetection[]`/…, or a text list) | `item` (the element, any type), `index`; exec: `loop_body` / `completed` | `items` (CSV `a,b` or JSON `["a",1]` when not wired) | Iterates the array **synchronously**: `loop_body` once per element (with `item`+`index`), then `completed` once. Empty → just `completed`. | `Person detection → (persons) → For each →loop_body Break person detection; →completed Print("done")` |
| **Flip flop** | — | exec: `a` / `b`; `is_a` (bool) | — | Alternates exec between `a` and `b` on each graph run (cross-run persistent). `is_a = true` when `a` just fired. Like Unreal's Flip Flop. | `Motion Watch → If(moved) → Flip flop →a Create alert("started") →b Create alert("still active")` |
| **Compare** | `a` (num), `b` (num) | `result` (bool) | `operator` (`>` `>=` `<` `<=` `==` `!=`, default `==`), `a` (default 0), `b` (default 0) | Compares two integers; `result` feeds `If`. Either operand can be wired from upstream **or** typed as a literal in the inspector. | `Person detection → (count) a → Compare(operator=">", b=0) → (result) → If →true Notify` |
| **Text compare** | `a` (Text), `b` (Text) | `result` (bool) | `operator` (`eq` / `ne` / `contains` / `starts_with` / `ends_with`, default `eq`), `b` (literal fallback) | Compares two text values. `b` can be wired or typed as a literal in the inspector. | `Describe image → (description) a → Text compare(operator=contains, b=person) → If(result)` |
| **And** | `a` (bool), `b` (bool) | `result` (bool) | `a`, `b` (literal fallbacks) | `a && b` — pure logic, no exec pins. | `Compare → And → If(result)` |
| **Or** | `a` (bool), `b` (bool) | `result` (bool) | `a`, `b` | `a \|\| b` — pure logic. | `PersonDetection(ok) → Or → If(result)` |
| **Not** | `a` (bool) | `result` (bool) | `a` | `!a` — inverts a bool. | `Compare → Not → If(result)` |
| **Cooldown** | — | `elapsed_secs` (num), `ok` (bool) | `window_secs` (default 60) | Rate-limit gate: lets exec through only when at least `window_secs` have elapsed since it last fired. Persists state across runs. `ok = true` means the gate opened. | `OnEvent(motion) → Cooldown(window_secs=300) → Notify recipient` |
| **Wait** | — (exec only) | — | `seconds` | Delays the run by N seconds, staying active until it elapses. | `Create alert → Wait(5s) → Deliver alert` |
| **Emit event** | — (exec only) | — | `event` | Fires a named in-graph event → concurrent guarded `On event` handler; **dropped if that handler is still running**. Exec passes through. | `If(ok) → Emit event("seen")` |
| **On event** | — | exec: `exec_out` (entry) | `event` | Entry for a named event. **Never runs on the normal trigger** — only when a matching `Emit event` fires. | `On event("seen") → Get current frame → Recognize person` |
| **Print** | `message`, `on_screen` (bool) | — | `message`, `on_screen`, `level` | Prints to the editor console (+ on-screen flash unless off). Debug sink. | `Recognize person → Print("Saw {p1}", message=person)` |
| **Show image** | `image*` | — | `label` | Renders the input frame on the canvas. Debug sink; emits no data. | `Camera snapshot → Show image` |

### Utility

| Node | Inputs | Outputs | Config | Behavior | Sample usage |
|---|---|---|---|---|---|
| **Comment** | — | — | `text`, `color` (#RRGGBB, default `#facc15`), `width` (px, default 400), `height` (px, default 200) | Visual group box for annotating the canvas. Drag to reposition; nodes whose center falls inside move with it. Drag the bottom-right grip to resize. No ports, no execution. | *(canvas organizer — never part of execution flow)* |

---

## How to use events (`Emit event` / `On event`)

Events let a fast path trigger a slower, decoupled handler without blocking it or piling up.

1. **Define the handler.** Add an **On event** node, set its `event` to a name, e.g.
   `person_seen`. Wire its `exec_out` into a **self-contained** subtree — it must
   re-acquire its own data (events carry no payload in v1):
   `On event("person_seen") → Get current frame → Recognize person → Notify recipient`.
2. **Fire it.** On your main flow, add an **Emit event** node with the **same** `event`
   name on the exec chain, e.g. inside a per-person loop:
   `Cron 1s → Get current frame (memoize off) → Person detection → (persons) → For each
   →loop_body Emit event("person_seen")`.
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
      │ persons (PersonDetection[])
      ▼
[For each] ──loop_body──▶ [Emit event "person_seen"]
      └─completed─▶ (idle)

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

The 1 Hz loop stays cheap (warm frame + local person detection); it fires `person_seen`
per detected person, and the heavier recognize-and-notify handler runs concurrently —
never more than one at a time (repeat fires coalesce).

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
4. Engine-level special behavior (like `If` / `ShowImage` / `Emit event` / `For each`) lives
   in `engine.rs` (`forward_epoch`), not in `run_node`.
