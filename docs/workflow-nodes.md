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
available yet). Every `Emit event` also records a `workflow.custom` occurrence on the agnostic
event stream (card #66) — so it doubles as the "push a custom event to the timeline" node; the
event name + payload land in the feed with the emitting graph as `source`.

### Triggers (graph-level, not nodes)
How a whole graph starts a run (set in the graph header, not a node):
- **Manual** — only via Run-now.
- **Cron { every_secs }** — a fresh run every N seconds (a stopwatch, no wall-clock time).
- **Schedule** — a **calendar** schedule (#111): hourly at a minute, daily/weekly/monthly/yearly at
  chosen wall-clock times, in a timezone. Durable via the `work_jobs` queue (survives a flow-pod
  restart); next-occurrence math is `app_core::workflows::schedule` (chrono-tz, DST-correct). Built
  from dropdowns in the graph header — no cron syntax.

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
| **Break person detection** | `person` (Person) | `bbox` (Bbox), `confidence` % | — | Destructures a `PersonDetection` element into its fields (Unreal-style "Break"). | `For each(persons) → item → Break person detection → bbox / confidence` |
| **Break detection** | `detection` (Person) | `bbox` (Bbox), `confidence` % | — | Destructures a generic `Detection` element (from Detect objects) into its fields. Same as Break person detection but paired with the Detect objects node. | `For each(detections) → item → Break detection → bbox / confidence` |
| **Break face detection** | `face` (Face) | `bbox` (Bbox), `confidence` %, `landmarks` (count) | — | Destructures a `FaceDetection` element into its fields. | `For each(faces) → item → Break face detection` |
| **Recognize person** | `image*` | `person`, `person_id`, `confidence` %, `ok` | `max_distance` (0.4) | Embeds the largest face (AI sidecar) + kNN over enrolled people (pgvector). `ok` = match within threshold. All-in-one; the pipeline nodes below split it for storing unknowns + top-K. | `Camera snapshot → Recognize person → If(ok) → Notify recipient` |
| **Extract Face Embedding** | `image*` (Image), `bbox` (Bbox, optional) | `embedding` (**Embedding**), `dim` (Num), `ok` | `timeout_secs` (30) | Face recognition pipeline (#117) — the ONLY node that takes the frame: extracts a 512-d ArcFace embedding via the AI sidecar (model/dim abstraction). `bbox` reserved for a future crop-to-face; v1 embeds the most prominent face. | `On event("face_detected") → (frame) Extract Face Embedding → (embedding) → …` |
| **Recognize Face** | `embedding*` (**Embedding**) | `person`, `person_id`, `confidence` %, `matches` (**Dictionary** label→score), `found` | `top_k` (5), `max_distance` (0.5) | kNN over the **union gallery** — enrolled `person_embeddings` + all stored `faces` (labeled + unknown), each hit tagged by source. Takes the **embedding, not the image**. `matches` is the top-K as a Dictionary; `person_id` wires into Store Face's `label`. | `Extract Face Embedding → (embedding) Recognize Face(top_k=5) → If(found) → …` |
| **Store Face** | exec, `embedding*` (**Embedding**), `image` (Image), `bbox` (Bbox), `label` (Text — person id, empty=unknown), `camera` (Camera) | `stored` (Bool — false if deduped), `face_id` (Text) | `source`, `dedup_distance` (0.15) | Persists a face to the tenant store: image → MinIO, embedding + metadata → the `faces` table. Labeled (wire `Recognize Face.person_id` into `label`) or **unknown** (empty `label`). Skips near-duplicate embeddings (cosine ≤ `dedup_distance`). | `Recognize Face →(person_id) Store Face(label=person_id)` · unknown: `Store Face(label="")` |
| **Describe image** | `image*`, `question` (Text, optional) | `description`, `ok` | `label`, `question` | Natural-language caption of the frame via the vision model. Wire/set an optional `question` and the answer addresses it about the frame (e.g. "how many people?", "is the door open?") instead of a generic caption. | `Camera snapshot → Describe image(question="Is anyone at the door?") → Notify(message=description)` |
| **Motion detection** | `image_a*`, `image_b*` | `moved` (bool), `score` %, `ok` | `sensitivity`, `pixel_threshold` | Compares two frames; reports whether/how much they differ. | `snapshot A + snapshot B → Motion detection → If(moved)` |
| **Draw BBox** | `image*` (Image), `bbox*` (Bbox) | `image` (Image), `bbox` (the drawn/expanded box), `ok` | `color` (#RRGGBB, default `#00FF00`), `thickness` (px, default `2`), `style` (`box`/`corners`/`reticle`, default `box`), `expand` (% grow per side, default `0`) | Draws a bounding box on the image → annotated JPEG. `expand` grows the box outward by N% on each side (the grown box is also emitted on the `bbox` output for downstream reuse). `style`: `box` = full rectangle, `corners` = HUD L-brackets, `reticle` = brackets + centre crosshair (targeting look). Takes a typed `Bbox` (canonical xyxy) from any Break* node or `OnEvent("motion")` — no format toggle needed. | `Person detection → For each → Break person detection → (bbox) Draw BBox(style=reticle) → Show image` |
| **Draw Text** | `image*` (Image), `bbox*` (Bbox), `text` (Text) | `image` (Image), `ok` | `text` (literal fallback), `position` (`top-left`/`top-right`/`bottom-left`/`bottom-right`), `size` (px, default `18`), `text_color` (#RRGGBB, default `#FFFFFF`), `background` (bool, default on), `bg_color` (#RRGGBB, default `#000000`) | Draws a text label anchored to a corner of the bbox, optionally on a filled background rectangle in a contrasting colour. `text` is wired or a config literal. | `Detect People → For each → Break person → (bbox) Draw Text(text="Person", position=top-left) → Show image` |
| **Blur Outside Box** | `image*` (Image), `bbox*` (Bbox) | `image` (Image), `ok` | `blur` (Gaussian σ, default `12`) | Blurs everything *outside* the bounding box — the region inside stays sharp → redacted JPEG. Privacy-preserving framing (e.g. blur the scene, keep only the person). Takes a typed `Bbox` from any Break* node or `OnEvent("motion")`. | `Person detection → For each → Break person detection → (bbox) Blur Outside Box → Show image` |
| **AI If** | `image` (Image, optional), `question` (Text, optional) | exec: `true`, `false` | `question` (string), `timeout_secs` (default `65`) | Asks the vision model a yes/no question about an optional image. Routes exec to `true` or `false` exactly like `If` but powered by Claude. `question` can be wired (Text) or set as a config literal; no image wired = text-only question (e.g. "Is it after 18:00?"). Fails the run if no question is provided. | `Camera snapshot → AI If(question="Is there smoke?") →true Create alert` |
| **AI Answer** | `prompt*` (Text), `image` (Image, optional) | `response` (Text), `ok` | `prompt` (literal fallback), `timeout_secs` (default `30`) | Single-turn Claude — a free-text prompt (+ optional image for vision) → a plain-text answer. No tool-use loop, so it's fast enough for a chat reply. Complements **AI If** (yes/no routing) and **Describe image** (fixed caption). Needs `ANTHROPIC_API_KEY`. | `On event("telegram_message") → (args) → AI Answer(prompt=args) → (response) → Reply to sender` |
| **AI Extract** | `text*` (Text) | `value` (**typed per `type`**), `found` (Bool) | `field` (what to extract, e.g. "the camera name"), `type` (Text / Camera / Recipient / Number), `timeout_secs` (default `30`) | Slot/entity extraction — the NLU companion to **AI Switch**. One LLM call pulls `field` out of the message and resolves it to `type`: a **Camera** name → a real Camera entity (wires straight into Camera snapshot), a **Recipient** name → a Recipient, Number/Text parsed. `found` is false when nothing matched (or a camera name didn't resolve) → branch to a clarification. Needs `ANTHROPIC_API_KEY`. | `AI Switch →snapshot AI Extract(field="camera name", type=Camera) → (value) Camera snapshot → Reply to sender` |
| **Region** | `camera` (Camera, optional — wire override, else config `camera`) | `region` (**Region**) | `camera` (name/ip/"first"), `name`, `points` (drawn on a snapshot in the property panel) | Pure/reusable: a named line or zone. 2 points = a line, 3+ = a zone — which it means is decided by whichever `On event` reads it (card: edge tracking). Wire the SAME `Region` node into several `On event`s so they share one definition instead of each re-drawing it. | `Region(name="counter", points=[...]) → region → On event("object_entered").region` + `On event("object_exited").region` |
| **Get tracked objects** | `camera*` | `objects` (**TrackedObject[]**), `count` | `camera` (name/ip/"first"), `timeout_secs` (default `10`) | Pull-based "state of the world": every currently-tracked entity on a camera right now (uuid, class, bbox, zone if inside one) — the query counterpart to `On event("object_entered"/"object_exited"/"line_crossed")`'s transition events. Use this when a graph wants to check state on its own schedule (e.g. a Cron) instead of reacting to a change. | `Cron 5s → Get tracked objects(camera="Front") → (objects) For each → Break tracked object` |
| **Break tracked object** | `object*` (TrackedObject) | `uuid`, `class`, `bbox` (Bbox), `zone` | — | Destructures a `TrackedObject` element (from **Get tracked objects**' `objects` array) into its fields — the Break node this list was missing at first. `zone` is empty unless the object is currently inside a zone. | `For each(objects) → item → Break tracked object → (bbox) Draw BBox → Show image` |

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

### Telegram (bot brain)

The bot's reply side. A paired user's message fires **`On event("telegram_message")`** (a system
event, outputs `text`, `chat_id`, `from_name`, `command`, `args`); these nodes reply to *that*
chat — the counterpart to **Notify recipient**, which addresses configured recipients. Everything is
queued through the durable notification queue (card #57), so a reply survives a restart; the node
reports `queued`, not delivered. Only paired chats (per tenant) ever fire the event.

| Node | Inputs | Outputs | Config | Behavior | Sample usage |
|---|---|---|---|---|---|
| **Reply to sender** | `message*`, `chat_id*` (Num), `image` (Image, optional) | `queued` (bool), `result` | `message` | Replies to the chat that triggered the run. Wire `chat_id` from `On event("telegram_message")`. Optional `image` sends a photo with the text as caption. | `On event("telegram_message") → (command) String switch → /snap → Camera snapshot → (image) Reply to sender(chat_id)` |
| **Reply with recording** | `clip_id*` (Text), `chat_id*` (Num), `caption` (Text, optional) | `queued` (bool), `result` | `caption` | Fetches a recording clip from the tenant's edge and relays it to the sender as a video (≤50 MB). Wire `clip_id` from **Start recording**. | `Start recording … Stop recording → (clip_id) Reply with recording(chat_id, caption="Here's the clip")` |

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
| **AI Switch** | `text*` (Text) | exec: `<arm_1>`, `<arm_2>`, … , `default` | `arms` (list of intent labels), `timeout_secs` (default `30`) | Semantic router: **String switch, but the arm is chosen by a model from the message's *meaning*** — not an exact match. One constrained LLM call classifies `text` into the best-fitting arm; the built-in `default` fires when none is a confident match (or on any error, so the graph never dead-ends). Arms are user-defined in the inspector, exactly like String switch. Give arms meaningful labels ("take a photo", "answer a question"). Needs `ANTHROPIC_API_KEY`. Pair after a `String switch(command)` so explicit `/commands` route for free and only plain-English hits the model. | `On event("telegram_message") → (text) → AI Switch (snapshot / question / status) →question Camera snapshot → AI Answer → Reply to sender` |
| **For each** | `items` (**any array** — `String[]`/`Number[]`/`PersonDetection[]`/…, or a text list) | `item` (the element, any type), `index`; exec: `loop_body` / `completed` | `items` (CSV `a,b` or JSON `["a",1]` when not wired) | Iterates the array **synchronously**: `loop_body` once per element (with `item`+`index`), then `completed` once. Empty → just `completed`. | `Person detection → (persons) → For each →loop_body Break person detection; →completed Print("done")` |
| **Flip flop** | — | exec: `a` / `b`; `is_a` (bool) | — | Alternates exec between `a` and `b` on each graph run (cross-run persistent). `is_a = true` when `a` just fired. Like Unreal's Flip Flop. | `Motion Watch → If(moved) → Flip flop →a Create alert("started") →b Create alert("still active")` |
| **Compare** | `a` (num), `b` (num) | `result` (bool) | `operator` (`>` `>=` `<` `<=` `==` `!=`, default `==`), `a` (default 0), `b` (default 0) | Compares two integers; `result` feeds `If`. Either operand can be wired from upstream **or** typed as a literal in the inspector. | `Person detection → (count) a → Compare(operator=">", b=0) → (result) → If →true Notify` |
| **Text compare** | `a` (Text), `b` (Text) | `result` (bool) | `operator` (`eq` / `ne` / `contains` / `starts_with` / `ends_with`, default `eq`), `b` (literal fallback) | Compares two text values. `b` can be wired or typed as a literal in the inspector. | `Describe image → (description) a → Text compare(operator=contains, b=person) → If(result)` |
| **And** | `a` (bool), `b` (bool) | `result` (bool) | `a`, `b` (literal fallbacks) | `a && b` — pure logic, no exec pins. | `Compare → And → If(result)` |
| **Or** | `a` (bool), `b` (bool) | `result` (bool) | `a`, `b` | `a \|\| b` — pure logic. | `PersonDetection(ok) → Or → If(result)` |
| **Not** | `a` (bool) | `result` (bool) | `a` | `!a` — inverts a bool. | `Compare → Not → If(result)` |
| **Cooldown** | — | `elapsed_secs` (num), `ok` (bool) | `window_secs` (default 60) | Rate-limit gate: lets exec through only when at least `window_secs` have elapsed since it last fired. Persists state across runs. `ok = true` means the gate opened. | `OnEvent(motion) → Cooldown(window_secs=300) → Notify recipient` |
| **Delay Trigger** | — (exec only) | exec: `start`, `stop` | `wait_secs` (10), `extend_secs` (10) | Leading+trailing debounce (Unreal's Retriggerable Delay, plus a leading pulse). `start` fires on the **first** signal of an idle window; further signals only extend the window; `stop` fires once `wait_secs` pass with **no** signal, and the node re-arms. So one burst of input = exactly one `start`…`stop` pair, and `Start recording` is called once per clip instead of on every event. Never blocks: the trailing `stop` arrives as its own run, scheduled durably, so an armed window survives a flow restart. | `OnEvent(motion) → Delay Trigger; start → Start recording; stop → Stop recording` |
| **Wait** | — (exec only) | — | `seconds` | Delays the run by N seconds, staying active until it elapses. | `Create alert → Wait(5s) → Deliver alert` |
| **Emit event** | — (exec only) | — | `event` | Fires a named in-graph event → concurrent guarded `On event` handler; **dropped if that handler is still running**. Exec passes through. | `If(ok) → Emit event("seen")` |
| **On event** | — | exec: `exec_out` (entry) | `event` | Entry for a named event. **Never runs on the normal trigger** — only when a matching `Emit event` fires. | `On event("seen") → Get current frame → Recognize person` |
| **Print** | `message`, `on_screen` (bool) | — | `message`, `on_screen`, `level` | Prints to the editor console (+ on-screen flash unless off). Debug sink. | `Recognize person → Print("Saw {p1}", message=person)` |
| **Show image** | `image*` | — | `label` | Renders the input frame on the canvas. Debug sink; emits no data. | `Camera snapshot → Show image` |

### Utility

| Node | Inputs | Outputs | Config | Behavior | Sample usage |
|---|---|---|---|---|---|
| **Comment** | — | — | `text`, `color` (#RRGGBB, default `#facc15`), `width` (px, default 400), `height` (px, default 200) | Visual group box for annotating the canvas. Drag to reposition; nodes whose center falls inside move with it. Drag the bottom-right grip to resize. No ports, no execution. | *(canvas organizer — never part of execution flow)* |

### Display

Content rendered **on the canvas**. **Title / Paragraph / Markdown** are pure decoration (no ports,
no execution) for documenting a graph. **Text / Gauge** are live sinks (like `Show image`, but for a
small value): `exec_in` + an optional `value` input; exec passes through, no data out. When exec
reaches them they persist + display the value (survives refresh); unwired, they show their config
literal. Each config value is editable in the property panel.

| Node | Inputs | Outputs | Config | Behavior | Sample usage |
|---|---|---|---|---|---|
| **Title** | — | — | `text`, `color` (#RRGGBB), `size` (`sm`/`md`/`lg`/`xl`) | A big heading drawn on the canvas. Auto-sizes to the text. | *(label a section of the flow)* |
| **Paragraph** | — | — | `text`, `color`, `width`, `height` | A resizable box of wrapped multiline text for a long description / help note. Drag the bottom-right grip to resize. | *(document what a subtree does)* |
| **Markdown** | — | — | `text`, `width`, `height` | A resizable box that renders `text` as basic Markdown (headings `#`/`##`/`###`, bullets `-`/`*`) — SVG-native, no HTML embed. | *(a rich doc block on the canvas)* |
| **Text** | `value` (Text, optional) | — | `value` (literal fallback), `bg_color`, `text_color` | Live sink: shows a wired Text value (or the literal) in a coloured box under its ports. | `Recognize person → (person) Text` (live name readout) |
| **Gauge** | `value` (Num, optional) | — | `value` (literal), `min`, `max`, `label`, `color`, `size` | Live sink: shows a wired Num as a semicircular gauge arc + number. | `Person detection → (count) Gauge(min=0, max=20)` |

### Variables

Declare typed, per-graph variables in the editor's left **Variables** panel (name + type, reusing
the event-field type dropdown). Each variable spawns two nodes (added from the panel's Get/Set
buttons, not the generic palette). Scope is **one run** — a variable read before any Set yields its
type's zero value. The node's port type follows the variable's declared type.

| Node | Inputs | Outputs | Config | Behavior | Sample usage |
|---|---|---|---|---|---|
| **Get Variable** | — | `value` (the variable's type) | `variable`, `type` (baked from the declaration) | **Pure** (no exec pins) — drop anywhere, wire its `value` out, no flow wires needed. Reads the variable's current value. | `Get Variable(count) → (value) → Compare(b=0)` |
| **Set Variable** | `value`* (the variable's type) | — | `variable`, `type` | Writes `value` into the variable; **exec passes through** so it writes at an exact point in the flow. | `Person detection → (count) → Set Variable(count) →exec next` |

---

### Dictionaries

A **Dictionary** is a named, tenant-scoped, **typed-column table** (columns-with-types = structure;
data rows = typed values), created/edited in the **Dictionaries** view and reused across workflows.
A variable of type **Dictionary** (`table`) references one by id. Every node below takes a `table`
input — a dropdown-picked dictionary id, or a wire from a `Get Variable(<Table var>)`. A `row` is
read/written as a `colname → value` string dictionary. **Read** nodes are pure and work off a
run-start snapshot; **mutate** nodes are exec passthroughs that hit the live DB row under a
`SELECT … FOR UPDATE` lock — so concurrent runs mutating the same dictionary are **atomic + thread-safe**.

| Node | Inputs | Outputs | Config | Behavior | Sample usage |
|---|---|---|---|---|---|
| **Dictionary Row Count** | `table`* | `count` (Num) | `table` | **Pure.** Number of rows. | `Dict Row Count(orders) → (count) → Compare(b=0)` |
| **Dictionary Get Row** | `table`*, `index`* (Num) | `row` (Dict), `found` (Bool) | `table`, `index` | **Pure.** The row at `index`. | `Get Row(hours, index=0) → (row) → Format` |
| **Dictionary Find Row** | `table`*, `column`* (Text), `key`* (Text) | `row` (Dict), `index` (Num), `found` (Bool) | `table`, `column`, `key` | **Pure.** First row whose `column` cell equals `key` (get-first-by-key). | `Find Row(people, column=name, key=Ana)` |
| **Dictionary Get Cell** | `table`*, `row`* (Num), `column`* (Text) | `value` (Text), `found` (Bool) | `table`, `row`, `column` | **Pure.** One cell. | `Get Cell(hours, row=0, column=open)` |
| **Dictionary Add Row** | `table`*, `values` (Dict colname→value) | — | `table`, `values` | **Mutate** (atomic). Append a row, positioned + coerced to the schema. | `motion → Add Row(log, values={cam:$cam})` |
| **Dictionary Set Cell** | `table`*, `row`* (Num), `column`* (Text), `value` (Text) | — | `table`, `row`, `column`, `value` | **Mutate** (atomic). Set one cell. | `Set Cell(state, row=0, column=armed, value=true)` |
| **Dictionary Update Row** | `table`*, `column`* (Text), `key`* (Text), `values` (Dict) | — | `table`, `column`, `key`, `values` | **Mutate** (atomic). Replace the first row matching `key` in `column`. | `Update Row(people, column=id, key=7, values={seen:now})` |
| **Dictionary Delete Row** | `table`*, `column`* (Text), `key`* (Text) | — | `table`, `column`, `key` | **Mutate** (atomic). Delete the first row matching `key`. | `Delete Row(cart, column=sku, key=A1)` |
| **Dictionary Clear** | `table`* | — | `table` | **Mutate** (atomic). Remove all rows (keep the schema). | `nightly → Clear(daily_counts)` |

**Build a table in-graph** (pure; no DB — compose an unsaved table and pass it along a `table` wire):

| Node | Inputs | Outputs | Config | Notes | Example |
|---|---|---|---|---|---|
| **Dictionary Schema** | — | `schema` (**Table**) | `columns` = `[{name, type}]` | **Pure.** An inline, unsaved empty table — the seed for composing rows. `type ∈ text\|num\|bool\|camera\|recipient`. | `Dictionary Schema(cols=[day:text, open:text]) → (schema)` |
| **Create Dictionary Row** | `schema`* (**Table**) + **one input per column** (typed by the column's cell type) | `row` (**Dictionary** colname→value) | `columns` = `[{name, type}]` | **Pure.** Dynamic inputs mirror the columns. `schema` is a structural dependency — the column list is baked into config. | `Create Dictionary Row(schema, day=Mon, open=09:00) → (row)` |
| **Add Dictionary Row** | `table`* (**Table**), `row`* (**Dictionary**) | `table` (**Table**) | — | **Pure** (in-memory, no DB). Coerce the `row` to the `table`'s schema and append it; emit the new `table`. | `Dictionary Schema → Create Row → Add Row → (table) → Dict Row Count` |

### Smart Home (Tuya)

Control smart-home devices (switches, plugs, lights) linked to the operator's single Tuya Cloud
project. Devices are linked once (QR "Tuya App Account Authorization") and synced into the tenant's
catalog from **Settings → Integrations**; the node dropdowns list them **by name** (stored by stable
`device_id`). The `value` widget is typed to the chosen data point (Boolean → on/off switch, Enum →
select, Integer → number). Credentials are operator-level (`TUYA_ACCESS_ID`/`TUYA_ACCESS_KEY` env).

| Node | Inputs | Outputs | Config | Behavior | Sample usage |
|---|---|---|---|---|---|
| **Control Tuya Device** | — | `ok`, `response` (Text) | `tuya_device` (dropdown), `tuya_code` (data point, e.g. `switch_1`), `value` (typed to the point) | Sends one command to the device via the Tuya Cloud. Exec passes through. | `On event(motion) → Control Tuya Device(Entrada, switch_1, on)` |
| **Read Tuya Status** | — | `value` (Text), `on` (Bool), `raw` (Text JSON) | `tuya_device`, `tuya_code` | Reads the current value of one data point (`on` is a convenience for Boolean points; `raw` is the full status). | `Read Tuya Status(Entrada, switch_1) → If(on) → …` |

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

**Gating by camera/region (card: edge tracking).** `object_entered`/`object_exited`/
`line_crossed` are a special case: `On event` gains `camera` + `region` inputs for them, and
those inputs **filter delivery**, not just display a value — see the dedicated section below.
This is the general pattern for "an event needs to be filtered before it triggers" — extend it
(a new input + a `Graph::resolve_*` helper, see `docs/ref/workflows.md`'s "Gated events") rather
than inventing a new node kind for the next thing that needs scoping.

---

## Object tracking (card: edge tracking)

The edge runs a continuous YOLOX + SORT tracker for every distinct `(camera, class)` pair your
**enabled** graphs actually reference — not a fixed always-on job, and not a dedicated node.
Tracking lifecycle is just **three more system events** your existing `On event` already knows
how to handle (like `motion`/`telegram_message`), plus one pull-based query node:

| Event / node | Fires when | Fields |
|---|---|---|
| `On event("object_entered")` | A tracked entity is first confirmed (whole frame, or entering a zone if scoped — see below) | `uuid`, `class`, `camera`, `bbox`, `count`, `zone` |
| `On event("object_exited")` | A tracked entity disappears (or leaves its zone) | `uuid`, `class`, `camera`, `dwell_secs`, `count`, `zone` |
| `On event("line_crossed")` | A tracked entity's centre crosses a wired 2-point **Region** | `uuid`, `class`, `camera`, `line`, `direction` (`forward`/`backward`) |
| **Get tracked objects** (node, camera input) | Called on demand (e.g. from a Cron) | `objects` (**TrackedObject[]**: `uuid`, `class`, `bbox`, `zone`), `count` |

`object_entered`/`object_exited`/`line_crossed` are events (react to a transition); **Get
tracked objects** is a node (ask "what's true right now"). Reach for whichever matches what
you're actually trying to express.

### Whole-frame vs zone-scoped enter/exit
By default `object_entered`/`object_exited` mean "anywhere in the camera's frame". Wire a
**Region** with **3+ points** into the `On event`'s `region` input and they instead mean
"entered/exited THIS polygon": `zone` becomes the region's name, `bbox` stops being meaningful
(the geometry that matters is the polygon, not a box), and `dwell_secs` on exit is time spent
**inside the zone**, not just anywhere on camera — this is the built-in answer to "how long was
someone in this area", no extra bookkeeping needed.

### Zone trigger (config on `On event`, only shown once a region concept applies)
- **Intersects** (default) — fires the moment the tracked bbox touches the polygon at all.
  Looser; catches a partial/edge entry sooner and releases later.
- **Centre inside** — fires only once the entity's centre point is inside the polygon.
  Stricter; ignores someone just brushing past the zone's edge.

### Reusing one Region across several nodes
A **Region** is pure data — draw it once (click points on a live camera snapshot in its
property panel), then wire its `region` output into every `On event` that needs that same
geometry, instead of redrawing it per node:

```
[Region(name="counter", points=[3+ pts])] ── region ──┬──▶ On event("object_entered").region
                                                       └──▶ On event("object_exited").region
```

2 points = a crossing line (meaningful to `line_crossed` only); 3+ points = a polygon zone
(meaningful to `object_entered`/`object_exited` only) — the Region node itself doesn't enforce
or care which; draw whichever shape the node you're wiring it into actually needs.

### Scoping to one camera (and reusing that too)
`camera` on `On event` works the same way as `region`: pick it in the node's own config, or wire
a **Get camera by name** into several `On event`s so they all scope to the same physical camera
without re-picking it per node. An **unscoped** `On event` (no camera set or wired) fires for
**every** camera's tracking events — filter downstream with `If`/`Text compare` on the `camera`
output instead, if that's what you actually want.

### Tracking more than one class on the same camera
`class` (any of the 80 COCO classes, default `person`) is a plain per-node dropdown — not
wireable, since there's no "GetClassByName" node to point at, just a fixed string. Add more than
one `On event` for the same camera with **different** classes and the edge runs one independent
tracker per `(camera, class)` pair — a `person` tracker and a `car` tracker on the same feed
never share or confuse uuids.

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

## Worked example: a Telegram command bot

The bot brain is just a graph. A paired user's message fires `On event("telegram_message")`;
route on `command`, reply to `chat_id`.

```
[On event("telegram_message")]   (Concurrency: Concurrent)
  ├─ command ─▶ [String switch: /snap, /ask, /status]
  │      ├─ /snap   ─▶ [Camera snapshot(camera=args)] ─▶ (image) [Reply to sender]
  │      ├─ /ask    ─▶ [AI Answer(prompt=args)] ─▶ (response) [Reply to sender]
  │      ├─ /status ─▶ [Format "Online ✓"] ─▶ [Reply to sender]
  │      └─ default ─▶ [Reply to sender "Try /snap <cam>, /ask <q>, /status"]
  └─ chat_id ────────────────────────────────────────────▶ (wired into every Reply to sender)
```

Only chats this tenant has paired (via `/pair`) ever fire the event, and the reply always goes
back to the sender's `chat_id` — never another tenant's. Set the reply nodes **memoize = off** if
the same graph handles many messages in a run.

---

## Worked example: alert on a perimeter line crossing

```
[Region(name="fence", camera="Backyard", points=[(0.0,0.5),(1.0,0.5)])]
      │ region
      ▼ (wired into the region input below)
[On event("line_crossed")]   camera = "Backyard", class = "person"
      │ uuid, line, direction
      ▼
[Format] "Someone crossed {p1} going {p2}"   (p1 = line, p2 = direction)
      ▼
[Create alert(level="warn")]
```

Draw the line once on a live snapshot in the Region node's property panel; `direction` tells you
which way (`forward`/`backward`, relative to the two points' own order) — pick the point order
so "forward" means whatever "in" is for this fence.

## Worked example: how long did someone dwell in a zone?

```
[Region(name="counter", camera="Front desk", points=[3+ pts around the counter])]
      │ region
      ├──────────────────────────────────────────────┐
      ▼                                               ▼
[On event("object_entered")]                  [On event("object_exited")]
  camera="Front desk", class="person"           camera="Front desk", class="person"
      │ uuid                                          │ uuid, dwell_secs
      ▼                                               ▼
[Print "Visitor {p1} arrived"]                [Format "Visitor {p1} stayed {p2}s"]
                                                       ▼
                                               [Create alert(message=text)]
```

Wiring the **same** Region into both the `entered` and `exited` listeners means they always
agree on which zone they're talking about. `dwell_secs` on `object_exited` is already the
answer to "how long were they in the zone" — no manual start/stop timestamp bookkeeping needed.
(It's in seconds; there's no arithmetic node yet to convert to minutes — for now, either work in
seconds or divide client-side when you read the alert.)

## Worked example: periodic occupancy check (pull instead of react)

```
Trigger: Cron { every_secs: 30 }

[Get tracked objects(camera="Warehouse")]
      │ count
      ▼
[Compare(operator=">", b=20)]
      │ result
      ▼
[If] ──true──▶ [Create alert(message="Over capacity", level="warn")]
```

No `On event` at all here — this graph just asks "how many are there right now" every 30
seconds. If you need to act on each individual tracked entity instead of just the total, loop
`objects` with **For each** → **Break tracked object** to get its `uuid`/`class`/`bbox`/`zone`:

```
[Get tracked objects(camera="Warehouse")]
      │ objects
      ▼
[For each] ──loop_body──▶ [Break tracked object] ──(bbox)──▶ [Draw BBox] ──▶ [Show image]
```

That's the quickest way to actually **see** what's tracked today: pair a snapshot of the same
camera with `Draw BBox` per tracked entity and `Show image` to preview it on the canvas while
the graph runs. There's no dedicated live-tracking dashboard page yet — this graph-based preview
(or reading the raw `GET /devices/:ip/tracks` response) is the way to inspect it in the
meantime.

## Worked example: two classes, one camera, no cross-talk

```
[On event("object_entered")]  camera="Driveway", class="person"  →  Notify recipient("Person at driveway")
[On event("object_entered")]  camera="Driveway", class="car"     →  Notify recipient("Car at driveway")
```

Two separate `On event` nodes, same camera, different `class`. The edge runs a `person` tracker
and a `car` tracker on that camera as two independent jobs — each with its own uuid space — so
a car parking never gets mistaken for a person entering, and vice versa.

---

## Adding a new node (for maintainers)

1. `domain.rs`: add the `NodeKind` variant + add it to `NodeKind::ALL`, and the
   `slug` / `label` / `category` / `input_ports` / `output_ports` / `default_config` arms
   (+ a port-table const if it has data ports).
2. `node.rs`: add a `run_node` match arm (and a `debug_summary` arm).
3. The editor palette + inspector pick it up automatically from the catalog. No TS change.
4. Engine-level special behavior (like `If` / `ShowImage` / `Emit event` / `For each`) lives
   in `engine.rs` (`forward_epoch`), not in `run_node`.
