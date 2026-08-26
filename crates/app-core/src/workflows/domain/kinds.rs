//! The catalog: every kind of node, and the ports each one declares.
//!
//! This is the product's vocabulary, and the longest file here for a good reason — a catalog is a
//! list, and a list read top to bottom is easier than the same list scattered across twelve files.
//! What varies between entries is only the ports and the defaults.
//!
//! `ALL` is the user-visible set, and the count comes from counting it. Hidden kinds — `Reroute` —
//! are registered but never appear in the palette, so a graph that already contains one still
//! loads.
//!
//! Changing a slug here breaks every saved graph that used it. The catalog snapshot test exists to
//! make that a failed build rather than a support ticket.

use super::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

// ---------------------------------------------------------------------------
// Node kinds + their port declarations
// ---------------------------------------------------------------------------

/// The atomic node set (v1). Each kind does exactly one thing; behaviour is composed
/// by linking. Triggers are NOT nodes — they are a property of the `Graph`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    /// Source: reads the most-recent alert from the store into typed outputs.
    GetLastAlert,
    /// Source: pick a notification recipient → a typed `Recipient` output.
    GetRecipient,
    CreateAlert,
    DeliverAlert,
    CameraSnapshot,
    /// Source: the most-recent frame from the camera's warm in-memory feed (~zero
    /// latency, no per-call round trip) — for high-rate loops (e.g. recognise every
    /// second). `CameraSnapshot` is the on-demand/quality path.
    GetCurrentFrame,
    /// Pure data: pick a camera by name from a dropdown and output it as a `Camera`
    /// value. No exec pins — activates whenever its output is demanded. Use this to
    /// inject a static camera reference into any part of a graph without exec flow.
    GetCameraByName,
    /// Pure data: destructure a `Camera` entity → `id`, `ip`, `name` Text outputs.
    /// Like Unreal's Break node — use this when you need the raw strings from a camera
    /// that arrived on a `Camera` typed wire (e.g. from `OnEvent("motion")`).
    BreakCamera,
    /// Pure data: a reusable named line/zone, drawn on a camera's live snapshot in the property
    /// panel (card: edge tracking). Output as one `Region` value — wire it into `OnEvent`'s
    /// `region` input (`object_entered`/`object_exited`/`line_crossed`) so several nodes share
    /// the exact same geometry instead of each re-drawing it. 2 points is a line, 3+ a zone; which
    /// it means is decided by whichever `OnEvent` reads it. No exec pins — activates whenever its
    /// output is demanded, like `GetCameraByName`.
    Region,
    /// Action: query the edge tracker's live "state of the world" for a camera — every
    /// currently-confirmed tracked entity (uuid, class, bbox, zone if inside one). The pull-based
    /// counterpart to the `object_entered`/`object_exited`/`line_crossed` *events*: "what's
    /// tracked right now", for a graph that wants to check state on its own schedule instead of
    /// reacting to a transition. Loop the `objects` output with `ForEach`.
    GetTrackedObjects,
    /// Destructure a `TrackedObject` element (from `GetTrackedObjects`' `objects` array) →
    /// `uuid`/`class`/`bbox`/`zone`. Same Unreal-style "Break" shape as `BreakPersonDetection`.
    BreakTrackedObject,
    DescribeImage,
    /// Renamed from `notify_beacon`; the alias keeps old `workflows.json` loadable.
    #[serde(alias = "notify_beacon")]
    NotifyRecipient,
    /// Pure text composition: substitutes `{p1..p3}` into a `template`.
    Format,
    /// Flow control: delays the run by `seconds`, staying active until it ends.
    Wait,
    /// Network: POST form data (`key1..3` = field names, `value1..3` = values) to a URL.
    HttpPost,
    /// Flow control: route the run down `true`/`false` exec branches on a Bool condition.
    If,
    /// Flow control: iterate a list (text: comma-separated or a JSON array). Fires
    /// `loop_body` once per element (with `item` + `index`), synchronously, then fires
    /// `completed` once after the last. A body node runs on every pass, as being inside a
    /// loop implies.
    ForEach,
    /// Vision: compare two frames and report whether/how much they differ.
    MotionDetection,
    /// Vision: a camera-bound motion detector — picks its own camera, grabs two frames a
    /// short gap apart, and reports `moved`/`score`. Put it in a Cron graph to watch a feed
    /// continuously; wire `moved` → If → Emit event to act only on motion.
    MotionWatch,
    /// Vision: detect whether a person is present in an image (via the AI sidecar).
    PersonDetection,
    /// Vision: detect any single COCO class in an image. The class is chosen from a
    /// dropdown in the property panel or overridden by wiring the `class` Text input.
    /// Outputs the same shape as `PersonDetection` (`count`, `confidence`, `detections`,
    /// `ok`) so it wires directly into `ForEach` + `BreakDetection`.
    ObjectDetection,
    /// Destructure a `PersonDetection` element → `bbox` + `confidence` (like Unreal's Break).
    BreakPersonDetection,
    /// Destructure a generic `Detection` element (from `ObjectDetection`) →
    /// `bbox` + `confidence`. Equivalent to `BreakPersonDetection` but paired with
    /// the `ObjectDetection` node for clarity.
    BreakDetection,
    /// Destructure a `FaceDetection` element → `bbox` + `confidence` + `landmarks`.
    BreakFaceDetection,
    /// Vision: match a face in an image against the enrolled people (pgvector kNN).
    RecognizePerson,
    // ---- Face recognition pipeline (#117): embed → recognize → store ----
    /// Extract a 512-d face embedding from an image (optionally cropped to a `bbox`). The
    /// model/dim abstraction — calls the AI sidecar. Exec + data (`embedding`/`dim`/`ok`).
    FaceEmbedding,
    /// kNN-recognize an embedding against the tenant's union gallery (enrolled + stored). Config
    /// `top_k`/`max_distance`; outputs the best `person`/`confidence`, a `matches` Dictionary, `found`.
    RecognizeFace,
    /// Store a face (image → MinIO, embedding → the `faces` table), labeled (a person id) or
    /// unknown (empty label). Dedup by embedding distance. Exec passthrough; outputs `stored`/`face_id`.
    StoreFace,
    /// Presence: record a categorized detection (person/known person/car/cat/dog/
    /// unknown) in a room — persisted and pulsed live on the map. Exec passes through.
    RecordPresence,
    /// Events: fire the named event → (re)runs the matching `OnEvent` subtree as a
    /// decoupled, concurrent sub-run. Dropped while that subtree is already running
    /// (coalesce). Exec passes through so the main flow continues. Carries no data (v1).
    EmitEvent,
    /// Events: entry point for a named event — its `exec_out` drives a handler subtree.
    /// It NEVER runs on the graph's normal trigger; only when a matching `EmitEvent` fires.
    OnEvent,
    /// Debug: print a `message` to the editor Console (always) and, unless `on_screen`
    /// is off, flash it as on-screen text over the canvas — like Unreal's Print String.
    Print,
    /// Debug: show an `image` on the editor canvas — a sink for eyeballing a frame as
    /// it flows through the graph. Like `Print`, but for pixels; emits no data.
    ShowImage,
    /// Flow control: alternates exec between `a` and `b` on each graph run, with
    /// cross-run persistent state. `is_a` (bool data output) tells which branch just
    /// fired — true = A. Like Unreal's Flip Flop node. Engine-driven (not `run_node`).
    FlipFlop,
    /// Data: compare two integers with a configurable operator → `result` bool.
    /// Both operands (`a`, `b`) can be wired from upstream or typed as literals in the
    /// inspector. Feeds directly into `If` for conditional routing.
    Compare,
    /// Data: `a < b` (Num × Num → Bool). Operator baked in — no config needed.
    NumLt,
    /// Data: `a <= b` (Num × Num → Bool).
    NumLte,
    /// Data: `a > b` (Num × Num → Bool).
    NumGt,
    /// Data: `a >= b` (Num × Num → Bool).
    NumGte,
    /// Data: `a == b` (Num × Num → Bool).
    NumEq,
    /// Data: `a != b` (Num × Num → Bool).
    NumNe,
    /// Data: cast a Num to its decimal Text representation.
    NumToText,
    /// Data: cast a Bool to "true" or "false" Text.
    BoolToText,
    /// Flow control: rate-limit gate — lets exec through only when at least `window_secs`
    /// have elapsed since the node last fired. Persists `last_fired_at` across runs via
    /// node state. Outputs `elapsed_secs` (how long since last fire, or window+1 on first
    /// run) and `ok` (true = gate opened). Engine-driven (not `run_node`).
    Cooldown,
    /// Vision: draw a bounding box rectangle on an image → annotated JPEG out.
    /// Accepts `xyxy` (x1,y1,x2,y2) or `xywh` (x,y,width,height) via `format` config.
    DrawBbox,
    /// Vision: draw a text label anchored to a corner of a bbox (top-left/top-right/
    /// bottom-left/bottom-right), optionally on a filled background rectangle in a contrasting
    /// colour. `text` is wired (Text) or a config literal. → annotated JPEG out.
    DrawText,
    /// Vision: blur everything OUTSIDE a bounding box → redacted JPEG out (the
    /// region inside the box stays sharp). Accepts `xyxy`/`xywh` via `format`;
    /// `blur` config sets the Gaussian strength.
    BlurOutsideBbox,
    /// Vision: return the pixel dimensions of an image — useful when the frame
    /// arrives via a custom event and width/height aren't already on the wire.
    GetImageSize,
    /// Logic: `a && b` — true only when both inputs are true.
    And,
    /// Logic: `a || b` — true when at least one input is true.
    Or,
    /// Logic: `!a` — inverts a bool.
    Not,
    /// Data: compare two text values with a configurable operator → `result` bool.
    /// `b` can be wired from upstream or typed as a literal in the inspector.
    /// Operators: `eq`, `ne`, `contains`, `starts_with`, `ends_with`.
    TextCompare,
    /// Flow control: routes exec to the arm whose label exactly matches the incoming
    /// `value` string. Arms are user-defined in the inspector (one or more labels).
    /// A built-in `default` arm fires whenever no arm label matches. Engine-driven (like
    /// `If`) — the exec routing happens in `forward_epoch`, not in `run_node`.
    StringSwitch,
    /// Flow control: routes exec to the arm whose camera config matches the incoming
    /// `camera` entity. Arms are user-defined in the inspector — each arm is a camera
    /// selected from a dropdown. A built-in `default` arm fires when no arm matches.
    /// Engine-driven (routing happens in `forward_epoch`).
    CameraSwitch,
    /// AI flow control: asks the vision model a yes/no question about an optional image
    /// and routes exec down `true` or `false` — exactly like `If` but powered by Claude.
    /// `question` can be wired (Text) or set as a config literal; `image` is optional
    /// (text-only questions like "Is it past 18:00?" work without an image).
    AskAi,
    /// Canvas organizer: a resizable colored annotation box that sits behind all nodes.
    /// Purely visual — zero exec, zero ports, never runs. Drag to reposition and pull
    /// enclosed nodes with it; drag the bottom-right grip to resize.
    Comment,
    /// Display: a big static heading drawn on the canvas. Config `text` + `color`. Zero exec,
    /// zero ports — pure documentation, like `Comment` but foreground and title-styled.
    Title,
    /// Display: a resizable multiline text block for long descriptions/help. Config `text`,
    /// `color`, `width`, `height`. Zero exec, zero ports.
    Paragraph,
    /// Display: a resizable box that renders `text` as Markdown. Zero exec, zero ports (static
    /// v1 — a live wired variant is a later enhancement).
    Markdown,
    /// Display (live sink): shows a wired `value` (Text) — or the config literal — in a box on the
    /// canvas. A sink like `ShowImage`: `exec_in` + `value`, exec passes through, no data out.
    Text,
    /// Display (live sink): shows a wired `value` (Num) as a gauge arc. `min`/`max`/`color`/`size`
    /// + a `label` in config. Same sink shape as `Text`.
    Gauge,
    /// Wire organizer: a tiny pure passthrough node (any → any) used to bend and route
    /// wires cleanly across the canvas, like Unreal's reroute node. Zero config.
    Reroute,
    /// Recording: tell the edge to start recording on the given `camera_ip`. Idempotent —
    /// if already recording, the request is a no-op. Outputs `clip_id` (Text) that can
    /// be passed to `StopRecording` to stop a specific clip.
    StartRecording,
    /// Recording: tell the edge to stop a recording started by `StartRecording`. Pass
    /// the `clip_id` from `StartRecording` to stop the correct clip. Exec passes through.
    StopRecording,
    /// Flow control: debounced countdown gate. First `arm` starts a `wait_secs` countdown;
    /// re-triggers only extend the wait when less than `extend_secs` remain
    /// (`new_remaining = max(remaining, extend_secs)`). Fires `out` once when the countdown
    /// expires with no pending extensions. Cross-run: the first run polls and blocks; later
    /// runs extend and return immediately. Use to hold open a recording window past the last
    /// motion event.
    TimeGate,
    /// Flow control: leading+trailing debounce (Unreal's Retriggerable Delay, plus a leading
    /// pulse). The first signal of an idle window fires `start` immediately; each further
    /// signal only extends the window; once `wait_secs` pass with no signal, `stop` fires
    /// once and the node re-arms. So one burst of input = exactly one `start` … `stop` pair.
    /// Unlike `TimeGate` it never blocks: `start` must reach its downstream in the triggering
    /// run, so the trailing `stop` is delivered by a later run forced at this node, scheduled
    /// on the durable `work_jobs` queue. Engine-driven (not `run_node`).
    DelayTrigger,
    /// Flow control: ask a PERSON, on their phone, before continuing. Pushes an authorization
    /// request to the addressed Recipient and returns WITHOUT taking a branch — the run ends,
    /// exactly as `DelayTrigger`'s trailing edge does, because a run cannot block on a human.
    /// The answer (or the TTL expiring) resumes at this node via `spawn_node_subrun` and fires
    /// one of `positive` / `negative` / `failure`.
    ///
    /// Agnostic by design: it gates an exec edge, not a device. Whatever hangs off `positive` is
    /// the sensitive action. A denial is an ANSWER (`negative`) and is the most informative thing
    /// this node produces; `failure` means NO HUMAN DECIDED — timed out, undeliverable, error.
    /// Folding those together is how "nobody answered" gets treated as "go ahead".
    DeviceGate,
    /// Variables: read a workflow variable's current value → one typed data output (`value`).
    /// Pure (no exec pins) — drop it anywhere and wire its output, no flow wires needed. The
    /// variable name + type are in config; the output port type follows the variable's type.
    GetVariable,
    /// Variables: write a value into a workflow variable. One typed data input (`value`) + an
    /// exec passthrough so it writes at an exact point in the flow. The variable name + type
    /// are in config; the input port type follows the variable's type.
    SetVariable,
    /// Variables: write SEVERAL workflow variables at once — one typed input per variable listed
    /// in `config.variables`, plus an exec passthrough. Pure sugar over a column of `SetVariable`
    /// nodes: same per-run map, same semantics, one node instead of eight. An unwired input
    /// leaves its variable alone (that is what makes the node reusable across branches).
    MultiSetVariable,
    /// Variables: read SEVERAL workflow variables at once — one typed output per variable listed
    /// in `config.variables`. Pure (no exec pins), like `GetVariable`.
    MultiGetVariable,
    /// Telegram: reply directly to the chat that triggered the run (the `chat_id` from
    /// `On event("telegram_message")`), not a configured recipient. Sends `message` + optional
    /// `image`. Everything is queued (durable) — outputs `queued` + the `result` confirmation.
    ReplyToSender,
    /// AI: single-turn Claude — a free-text `prompt` (+ optional `image`) → a `response` string.
    /// Complements `AskAi` (yes/no routing) and `DescribeImage` (image caption). No tool-use loop.
    AiAnswer,
    /// Telegram: reply to the triggering chat with a recording clip. Fetches the `clip_id` from the
    /// edge, relays it as a video with an optional `caption`. Queued (video is heavy).
    ReplyWithRecording,
    /// AI flow control: a semantic router. Like `StringSwitch`, but the arm is chosen by a model
    /// from the `text` input's *meaning* — routes exec to the best-matching user-defined arm, or the
    /// built-in `default` when none is a confident match. Arms are user-defined in the inspector.
    AiSwitch,
    /// AI: extract one typed value from a message (slot filling — the NLU companion to AiSwitch).
    /// `field` (config) describes what to pull out; `type` (config) sets the output wire type and
    /// resolution (a `camera` name → a real Camera entity, a `recipient` name → a Recipient, etc.).
    /// Outputs `value` (typed per `type`) + `found`. One LLM call.
    AiExtract,
    // ---- Dictionaries (#113): read + mutate a named typed-column table ----
    /// Dictionary (read): number of rows in a `table` → `count` (Num). Pure.
    DictRowCount,
    /// Dictionary (read): the row at `index` → `row` (a colname→value Dictionary) + `found`. Pure.
    DictGetRow,
    /// Dictionary (read): the first row whose `column` cell equals `key` → `row` + `index` +
    /// `found`. The "get first by key" lookup. Pure.
    DictFindRow,
    /// Dictionary (read): one cell at (`row`, `column`) → `value` (Text) + `found`. Pure.
    DictGetCell,
    /// Dictionary (mutate): append a row from a `values` map (colname→value), positioned + coerced
    /// to the table's schema. Atomic (row-locked). Exec passthrough.
    DictAddRow,
    /// Dictionary (mutate): set one cell at (`row`, `column`) to `value`. Atomic. Exec passthrough.
    DictSetCell,
    /// Dictionary (mutate): replace the first row whose `column` cell equals `key` with a `values`
    /// map. Atomic. Exec passthrough.
    DictUpdateRow,
    /// Dictionary (mutate): delete the first row whose `column` cell equals `key`. Atomic. Exec
    /// passthrough.
    DictDeleteRow,
    /// Dictionary (mutate): remove all rows (keep the schema). Atomic. Exec passthrough.
    DictClear,
    /// Dictionary (build): an inline, unsaved schema — `columns` from config → an empty
    /// `schema` Table. Pure. The seed for composing a table in-graph (no DB).
    DictSchema,
    /// Dictionary (build): a `row` (colname→value Dictionary) from one input per configured
    /// column, gated on a `schema` Table (structural). Pure.
    DictMakeRow,
    /// Dictionary (build): append a `row` to a `table` in memory → the new `table`. Pure; no DB.
    DictAppendRow,
    // ---- Smart home (Tuya) ----
    /// Smart home: send one command to a linked Tuya device — pick the device + data point
    /// (`code`, e.g. `switch_1`) from dropdowns and set a `value` typed to the point. Calls
    /// the Tuya Cloud with the operator's project credentials. Exec passes through; outputs
    /// `ok` + the raw `response`.
    TuyaControl,
    /// Smart home: read one data point's current value from a linked Tuya device → `value`
    /// (Text), `on` (Bool convenience for Boolean points), and the full `raw` status JSON.
    TuyaReadStatus,
}

// Port tables (static). `*` in the spec == `required: true`.
pub(super) static GET_LAST_ALERT_OUT: [Port; 7] = [
    p("id", types::NUM, false),
    p("level", types::TEXT, false),
    p("message", types::TEXT, false),
    p("room", types::TEXT, false),
    p("source", types::TEXT, false),
    p("created_at", types::TEXT, false),
    p("ok", types::BOOL, false),
];

pub(super) static CREATE_ALERT_IN: [Port; 2] = [
    p("message", types::TEXT, true),
    p("room", types::TEXT, false),
];
pub(super) static CREATE_ALERT_OUT: [Port; 3] = [
    p("id", types::NUM, false),
    p("result", types::TEXT, false),
    p("ok", types::BOOL, false),
];

pub(super) static RECORD_PRESENCE_IN: [Port; 8] = [
    p("room", types::TEXT, false),
    p("category", types::TEXT, false),
    p("label", types::TEXT, false),
    p("camera", types::CAMERA, false),
    p("bbox", types::BBOX, false), // [x1,y1,x2,y2] from BreakPersonDetection
    p("frame_w", types::NUM, false), // source image width in pixels
    p("frame_h", types::NUM, false), // source image height in pixels
    p("duration_secs", types::NUM, false),
];
pub(super) static RECORD_PRESENCE_OUT: [Port; 2] =
    [p("id", types::NUM, false), p("ok", types::BOOL, false)];

pub(super) static DELIVER_ALERT_IN: [Port; 1] = [p("id", types::NUM, true)];
pub(super) static DELIVER_ALERT_OUT: [Port; 2] =
    [p("result", types::TEXT, false), p("ok", types::BOOL, false)];

pub(super) static CAMERA_SNAPSHOT_IN: [Port; 1] = [p("camera", types::CAMERA, true)];
pub(super) static CAMERA_SNAPSHOT_OUT: [Port; 5] = [
    p("image", types::IMAGE, false),
    p("result", types::TEXT, false),
    p("width", types::NUM, false),
    p("height", types::NUM, false),
    p("ok", types::BOOL, false),
];

pub(super) static GET_TRACKED_OBJECTS_IN: [Port; 1] = [p("camera", types::CAMERA, true)];
pub(super) static GET_TRACKED_OBJECTS_OUT: [Port; 2] = [
    p("objects", types::TRACKED_OBJECT_ARRAY, false),
    p("count", types::NUM, false),
];

pub(super) static GET_CURRENT_FRAME_IN: [Port; 1] = [p("camera", types::CAMERA, true)];
pub(super) static GET_CURRENT_FRAME_OUT: [Port; 5] = [
    p("image", types::IMAGE, false),
    p("age_ms", types::NUM, false), // how old the returned frame is (0 = fresh capture)
    p("width", types::NUM, false),
    p("height", types::NUM, false),
    p("ok", types::BOOL, false),
];

pub(super) static DESCRIBE_IMAGE_IN: [Port; 2] = [
    p("image", types::IMAGE, true),
    // Optional guiding question — when wired/set, the caption answers it about the frame.
    p("question", types::TEXT, false),
];
pub(super) static DESCRIBE_IMAGE_OUT: [Port; 2] = [
    p("description", types::TEXT, false),
    p("ok", types::BOOL, false),
];

pub(super) static GET_RECIPIENT_OUT: [Port; 2] = [
    p("recipient", types::RECIPIENT, false),
    p("ok", types::BOOL, false),
];

// Device Gate: ask a person before continuing. Both inputs are `required` — which still allows
// authoring them in the panel, since a required input is only unsatisfied when it has neither an
// edge NOR a config literal. An unset Recipient or an empty prompt is an AUTHORING mistake and
// should fail the run loudly; it must not arrive as `failure`, which means a human was asked and
// did not answer. Hiding a broken gate behind the same arm as an unanswered one is how a graph
// ends up treating "never asked" as "asked and ignored".
pub(super) static DEVICE_GATE_IN: [Port; 2] = [
    p("recipient", types::RECIPIENT, true),
    p("message", types::TEXT, true),
];
// What the answer was, for graphs that want to log or notify on it. `factor` carries the map's
// decision 02: an approval confirmed by passcode rather than biometrics is still an approval, but
// its strength must be legible rather than assumed. Empty on the `failure` arm — nobody answered,
// so there is nothing to attribute.
pub(super) static DEVICE_GATE_OUT: [Port; 2] = [
    p("answered_by", types::TEXT, false),
    p("factor", types::TEXT, false),
];

pub(super) static NOTIFY_RECIPIENT_IN: [Port; 3] = [
    p("message", types::TEXT, true),
    p("recipient", types::RECIPIENT, false),
    p("image", types::IMAGE, false),
];
pub(super) static NOTIFY_RECIPIENT_OUT: [Port; 2] =
    [p("result", types::TEXT, false), p("ok", types::BOOL, false)];

pub(super) static FORMAT_IN: [Port; 3] = [
    p("p1", types::TEXT, false),
    p("p2", types::TEXT, false),
    p("p3", types::TEXT, false),
];
pub(super) static FORMAT_OUT: [Port; 1] = [p("text", types::TEXT, false)];

// HTTP POST: url + arbitrary header/field maps + an optional file (→ multipart).
pub(super) static HTTP_POST_IN: [Port; 4] = [
    p("url", types::TEXT, true),
    p("headers", types::DICTIONARY, false),
    p("fields", types::DICTIONARY, false),
    p("file", types::FILE, false),
];
pub(super) static HTTP_POST_OUT: [Port; 3] = [
    p("status", types::NUM, false),
    p("response", types::TEXT, false),
    p("ok", types::BOOL, false),
];

// Smart home (Tuya): device + data point are config dropdowns (no data inputs), like the
// camera picker on `GetCameraByName`.
pub(super) static TUYA_CONTROL_OUT: [Port; 2] = [
    p("ok", types::BOOL, false),
    p("response", types::TEXT, false),
];
pub(super) static TUYA_READ_STATUS_OUT: [Port; 3] = [
    p("value", types::TEXT, false),
    p("on", types::BOOL, false),
    p("raw", types::TEXT, false),
];

// ---- Dictionaries (#113) ----
// A `row` is emitted / consumed as a colname→value string Dictionary (consumable by Format,
// HTTP Post fields, etc.). The `table` port is a reference to a named dictionary (dropdown-picked
// id, or wired from a Table variable).
pub(super) static DICT_ROW_COUNT_IN: [Port; 1] = [p("table", types::TABLE, true)];
pub(super) static DICT_ROW_COUNT_OUT: [Port; 1] = [p("count", types::NUM, false)];

pub(super) static DICT_GET_ROW_IN: [Port; 2] =
    [p("table", types::TABLE, true), p("index", types::NUM, true)];
pub(super) static DICT_GET_ROW_OUT: [Port; 2] = [
    p("row", types::DICTIONARY, false),
    p("found", types::BOOL, false),
];

pub(super) static DICT_FIND_ROW_IN: [Port; 3] = [
    p("table", types::TABLE, true),
    p("column", types::TEXT, true),
    p("key", types::TEXT, true),
];
pub(super) static DICT_FIND_ROW_OUT: [Port; 3] = [
    p("row", types::DICTIONARY, false),
    p("index", types::NUM, false),
    p("found", types::BOOL, false),
];

pub(super) static DICT_GET_CELL_IN: [Port; 3] = [
    p("table", types::TABLE, true),
    p("row", types::NUM, true),
    p("column", types::TEXT, true),
];
pub(super) static DICT_GET_CELL_OUT: [Port; 2] = [
    p("value", types::TEXT, false),
    p("found", types::BOOL, false),
];

pub(super) static DICT_ADD_ROW_IN: [Port; 2] = [
    p("table", types::TABLE, true),
    p("values", types::DICTIONARY, false),
];
pub(super) static DICT_SET_CELL_IN: [Port; 4] = [
    p("table", types::TABLE, true),
    p("row", types::NUM, true),
    p("column", types::TEXT, true),
    p("value", types::TEXT, false),
];
pub(super) static DICT_UPDATE_ROW_IN: [Port; 4] = [
    p("table", types::TABLE, true),
    p("column", types::TEXT, true),
    p("key", types::TEXT, true),
    p("values", types::DICTIONARY, false),
];
pub(super) static DICT_DELETE_ROW_IN: [Port; 3] = [
    p("table", types::TABLE, true),
    p("column", types::TEXT, true),
    p("key", types::TEXT, true),
];
pub(super) static DICT_CLEAR_IN: [Port; 1] = [p("table", types::TABLE, true)];

// Build nodes (compose a table in-graph, no DB). DictSchema seeds an empty typed table from its
// config columns; DictMakeRow's per-column inputs are dynamic (see `dict_row_ports`) — this static
// list is the fallback (just `schema`); DictAppendRow appends a `row` Dictionary to a `table`.
pub(super) static DICT_SCHEMA_OUT: [Port; 1] = [p("schema", types::TABLE, false)];
pub(super) static DICT_MAKE_ROW_IN: [Port; 1] = [p("schema", types::TABLE, true)];
pub(super) static DICT_MAKE_ROW_OUT: [Port; 1] = [p("row", types::DICTIONARY, false)];
pub(super) static DICT_APPEND_ROW_IN: [Port; 2] = [
    p("table", types::TABLE, true),
    p("row", types::DICTIONARY, true),
];
pub(super) static DICT_APPEND_ROW_OUT: [Port; 1] = [p("table", types::TABLE, false)];

// If: a Bool condition routes the run down one of two exec branches.
pub(super) static IF_IN: [Port; 1] = [p("condition", types::BOOL, true)];

// For each: iterate ANY array (a wired `*Array`, or a text list authored in config).
// `items`/`item` are `Any` so one loop node handles every element type.
pub(super) static FOR_EACH_IN: [Port; 1] = [p("items", types::ANY, false)];
pub(super) static FOR_EACH_OUT: [Port; 2] =
    [p("item", types::ANY, false), p("index", types::NUM, false)];

// Object detection: detect any COCO class — class chosen by config or wired `class` input.
pub(super) static OBJECT_DETECTION_IN: [Port; 2] = [
    p("image", types::IMAGE, true),
    p("class", types::TEXT, false),
];
pub(super) static OBJECT_DETECTION_OUT: [Port; 6] = [
    p("count", types::NUM, false),
    p("confidence", types::NUM, false), // percent 0..100 (top detection)
    p("detections", types::PERSON_ARRAY, false), // one Detection per match — loop this
    p("width", types::NUM, false),
    p("height", types::NUM, false),
    p("ok", types::BOOL, false),
];

// Break (destructure) nodes — split a struct element into its fields, like Unreal's Break.
pub(super) static BREAK_PERSON_IN: [Port; 1] = [p("person", types::PERSON, true)];
pub(super) static BREAK_PERSON_OUT: [Port; 2] = [
    p("bbox", types::BBOX, false),
    p("confidence", types::NUM, false),
];
pub(super) static BREAK_TRACKED_OBJECT_IN: [Port; 1] = [p("object", types::TRACKED_OBJECT, true)];
pub(super) static BREAK_TRACKED_OBJECT_OUT: [Port; 4] = [
    p("uuid", types::TEXT, false),
    p("class", types::TEXT, false),
    p("bbox", types::BBOX, false),
    p("zone", types::TEXT, false),
];
pub(super) static BREAK_DETECTION_IN: [Port; 1] = [p("detection", types::PERSON, true)];
pub(super) static BREAK_DETECTION_OUT: [Port; 2] = [
    p("bbox", types::BBOX, false),
    p("confidence", types::NUM, false),
];
pub(super) static BREAK_FACE_IN: [Port; 1] = [p("face", types::FACE, true)];
pub(super) static BREAK_FACE_OUT: [Port; 3] = [
    p("bbox", types::BBOX, false),
    p("confidence", types::NUM, false),
    p("landmarks", types::NUM, false), // count of the 5-point landmark set
];

// Motion detection: compare two frames; report whether/how much they differ.
pub(super) static MOTION_IN: [Port; 2] = [
    p("image_a", types::IMAGE, true),
    p("image_b", types::IMAGE, true),
];
pub(super) static MOTION_OUT: [Port; 3] = [
    p("moved", types::BOOL, false),
    p("score", types::NUM, false),
    p("ok", types::BOOL, false),
];
pub(super) static MOTION_WATCH_IN: [Port; 1] = [p("camera", types::CAMERA, true)];

// Person detection: an image in → whether a person is present (via the AI sidecar).
pub(super) static PERSON_DETECTION_IN: [Port; 1] = [p("image", types::IMAGE, true)];
pub(super) static PERSON_DETECTION_OUT: [Port; 6] = [
    p("count", types::NUM, false),
    p("confidence", types::NUM, false), // percent 0..100 (top person)
    p("persons", types::PERSON_ARRAY, false), // one PersonDetection per person — loop this
    p("width", types::NUM, false),
    p("height", types::NUM, false),
    p("ok", types::BOOL, false),
];

// Recognize person: a face image in → the matched person + confidence.
pub(super) static RECOGNIZE_PERSON_IN: [Port; 1] = [p("image", types::IMAGE, true)];
pub(super) static RECOGNIZE_PERSON_OUT: [Port; 4] = [
    p("person", types::TEXT, false),    // matched person's name
    p("person_id", types::TEXT, false), // matched person's id
    p("confidence", types::NUM, false), // percent 0..100
    p("ok", types::BOOL, false),        // true when a match within threshold
];

// ---- Face recognition pipeline (#117) ----
pub(super) static FACE_EMBEDDING_IN: [Port; 2] = [
    p("image", types::IMAGE, true),
    p("bbox", types::BBOX, false), // optional crop-to-face (edge already found it)
];
pub(super) static FACE_EMBEDDING_OUT: [Port; 3] = [
    p("embedding", types::EMBEDDING, false),
    p("dim", types::NUM, false),
    p("ok", types::BOOL, false),
];
pub(super) static RECOGNIZE_FACE_IN: [Port; 1] = [p("embedding", types::EMBEDDING, true)];
pub(super) static RECOGNIZE_FACE_OUT: [Port; 9] = [
    p("person", types::TEXT, false), // best match's name ("" if none / unknown)
    p("person_id", types::TEXT, false), // best match's person id (wire → StoreFace.label)
    p("confidence", types::NUM, false), // percent 0..100 of the best single embedding
    p("matches", types::DICTIONARY, false), // top-K identities: person/label → score
    p("found", types::BOOL, false),  // a labeled match within max_distance AND unambiguous
    // #123 — per-identity scoring, so a graph can branch on HOW the match was won, not just that
    // it was. `confidence` is one lucky crop; `consistency` is the whole identity agreeing.
    p("consistency", types::NUM, false), // percent from the mean of the identity's 3 nearest
    p("samples", types::NUM, false),     // how many of that identity's embeddings were candidates
    p("margin", types::NUM, false),      // percentage points clear of the next DIFFERENT identity
    p("ambiguous", types::BOOL, false),  // runner-up within `margin` — found is forced false
];
pub(super) static STORE_FACE_IN: [Port; 12] = [
    p("image", types::IMAGE, false),
    p("embedding", types::EMBEDDING, true),
    p("bbox", types::BBOX, false),
    p("label", types::TEXT, false), // a person id; empty = unknown
    p("camera", types::CAMERA, false),
    p("confidence", types::NUM, false), // detection score 0..100 (from face_detected)
    p("face_width", types::NUM, false), // face box width in source px (far-face signal)
    // Quality signals (#126), carried through from `face_detected` so the stored capture records
    // what it was measured at. STORED, never judged — the gate that used them ran upstream.
    // The edge's 5 YuNet keypoints, if the graph wires them from `face_detected`. Used only as a
    // FALLBACK for the stored overlay when the sidecar cannot produce the dense 106 — the pose
    // columns are already computed from these on the edge, so nothing here recomputes them.
    p("kps", types::LANDMARKS, false),
    p("blur", types::NUM, false),  // sharpness; higher = sharper
    p("yaw", types::NUM, false),   // degrees
    p("pitch", types::NUM, false), // degrees
    p("roll", types::NUM, false),  // degrees
];
pub(super) static STORE_FACE_OUT: [Port; 2] = [
    p("stored", types::BOOL, false), // false if skipped by dedup
    p("face_id", types::TEXT, false),
];

// Print: a message to echo to the Console (+ optional on-screen flash).
pub(super) static PRINT_IN: [Port; 2] = [
    p("message", types::TEXT, false),
    p("on_screen", types::BOOL, false),
];

// Show image: a debug sink — an image in, flashed on the canvas. No data out.
pub(super) static SHOW_IMAGE_IN: [Port; 1] = [p("image", types::IMAGE, true)];
// Display sinks (Text/Gauge): one optional `value` input; unwired falls back to the config literal.
pub(super) static TEXT_DISPLAY_IN: [Port; 1] = [p("value", types::TEXT, false)];
pub(super) static GAUGE_DISPLAY_IN: [Port; 1] = [p("value", types::NUM, false)];

// Flip Flop: exec a/b outputs (alternating), plus is_a bool data output.
pub(super) static EXEC_OUT_FLIP_FLOP: [Port; 2] =
    [p("a", types::EXEC, false), p("b", types::EXEC, false)];
pub(super) static FLIP_FLOP_OUT: [Port; 1] = [p("is_a", types::BOOL, false)];

// Compare: two numeric inputs (wired or literal) → a bool result.
pub(super) static COMPARE_IN: [Port; 2] = [p("a", types::NUM, false), p("b", types::NUM, false)];
pub(super) static COMPARE_OUT: [Port; 1] = [p("result", types::BOOL, false)];

// Type cast nodes.
pub(super) static NUM_TO_TEXT_IN: [Port; 1] = [p("value", types::NUM, true)];
pub(super) static BOOL_TO_TEXT_IN: [Port; 1] = [p("value", types::BOOL, true)];
pub(super) static CAST_TEXT_OUT: [Port; 1] = [p("text", types::TEXT, false)];

pub(super) static COOLDOWN_OUT: [Port; 2] = [
    p("elapsed_secs", types::NUM, false),
    p("ok", types::BOOL, false),
];

pub(super) static DRAW_BBOX_IN: [Port; 2] =
    [p("image", types::IMAGE, true), p("bbox", types::BBOX, true)];
// `bbox` is the box actually drawn — i.e. the input box after the `expand` config is applied,
// so downstream nodes (blur, crop, presence) can reuse the expanded region.
pub(super) static DRAW_BBOX_OUT: [Port; 3] = [
    p("image", types::IMAGE, false),
    p("bbox", types::BBOX, false),
    p("ok", types::BOOL, false),
];

// DrawText: an image + a bbox to anchor against + the text (wired or config literal).
pub(super) static DRAW_TEXT_IN: [Port; 3] = [
    p("image", types::IMAGE, true),
    p("bbox", types::BBOX, true),
    p("text", types::TEXT, false),
];
pub(super) static DRAW_TEXT_OUT: [Port; 2] =
    [p("image", types::IMAGE, false), p("ok", types::BOOL, false)];

pub(super) static BLUR_OUTSIDE_BBOX_IN: [Port; 2] =
    [p("image", types::IMAGE, true), p("bbox", types::BBOX, true)];
pub(super) static BLUR_OUTSIDE_BBOX_OUT: [Port; 2] =
    [p("image", types::IMAGE, false), p("ok", types::BOOL, false)];

pub(super) static GET_IMAGE_SIZE_IN: [Port; 1] = [p("image", types::IMAGE, true)];
pub(super) static GET_IMAGE_SIZE_OUT: [Port; 3] = [
    p("width", types::NUM, false),
    p("height", types::NUM, false),
    p("ok", types::BOOL, false),
];

pub(super) static BOOL_OP_IN: [Port; 2] = [p("a", types::BOOL, false), p("b", types::BOOL, false)];
pub(super) static NOT_IN: [Port; 1] = [p("a", types::BOOL, false)];
pub(super) static TEXT_COMPARE_IN: [Port; 2] =
    [p("a", types::TEXT, false), p("b", types::TEXT, false)];

// String switch: one required text input (the value to match against the arms).
pub(super) static STRING_SWITCH_IN: [Port; 1] = [p("value", types::TEXT, true)];

// Camera switch: one required camera input (the entity to match against the arms).
pub(super) static CAMERA_SWITCH_IN: [Port; 1] = [p("camera", types::CAMERA, true)];

// AI switch: one required text input (the message to route by meaning across the arms).
pub(super) static AI_SWITCH_IN: [Port; 1] = [p("text", types::TEXT, true)];

// AI extract: one required text input (the message to pull a value from).
pub(super) static AI_EXTRACT_IN: [Port; 1] = [p("text", types::TEXT, true)];
// Static template outputs — the real `value` type is dynamic per config.type (GraphNode::output_ports).
pub(super) static AI_EXTRACT_OUT: [Port; 2] = [
    p("value", types::TEXT, false),
    p("found", types::BOOL, false),
];

// AI If: optional image + question text. Both are optional at the port level so a
// text-only question (no image wired) works. The node errors at runtime if no question.
pub(super) static ASK_AI_IN: [Port; 2] = [
    p("image", types::IMAGE, false),
    p("question", types::TEXT, false),
];

// Reroute: wildcard passthrough — one Any input, one Any output.
pub(super) static REROUTE_IN: [Port; 1] = [p("value", types::ANY, false)];
pub(super) static REROUTE_OUT: [Port; 1] = [p("value", types::ANY, false)];

// StartRecording: camera entity + optional pre-roll hint.
pub(super) static START_RECORDING_IN: [Port; 2] = [
    p("camera", types::CAMERA, true),
    p("pre_roll_secs", types::NUM, false),
];
pub(super) static START_RECORDING_OUT: [Port; 1] = [p("clip_id", types::TEXT, false)];

// StopRecording: camera entity + optional clip_id from StartRecording.
pub(super) static STOP_RECORDING_IN: [Port; 2] = [
    p("camera", types::CAMERA, true),
    p("clip_id", types::TEXT, false),
];

// Reply to sender (Telegram): text reply to the triggering chat + optional image.
pub(super) static REPLY_TO_SENDER_IN: [Port; 3] = [
    p("message", types::TEXT, true),
    p("chat_id", types::NUM, true),
    p("image", types::IMAGE, false),
];
pub(super) static REPLY_TO_SENDER_OUT: [Port; 2] = [
    p("queued", types::BOOL, false),
    p("result", types::TEXT, false),
];

// AI Answer: free-text prompt (required) + optional image → a text response.
pub(super) static AI_ANSWER_IN: [Port; 2] = [
    p("prompt", types::TEXT, true),
    p("image", types::IMAGE, false),
];
pub(super) static AI_ANSWER_OUT: [Port; 2] = [
    p("response", types::TEXT, false),
    p("ok", types::BOOL, false),
];

// Reply with recording (Telegram): a clip_id from StartRecording + chat_id + optional caption.
pub(super) static REPLY_RECORDING_IN: [Port; 3] = [
    p("clip_id", types::TEXT, true),
    p("chat_id", types::NUM, true),
    p("caption", types::TEXT, false),
];
pub(super) static REPLY_RECORDING_OUT: [Port; 2] = [
    p("queued", types::BOOL, false),
    p("result", types::TEXT, false),
];

pub(super) static GET_CAMERA_BY_NAME_OUT: [Port; 1] = [p("camera", types::CAMERA, false)];
/// `Region`'s `camera` input is optional/wireable (from `GetCameraByName`) with a config-picker
/// fallback — same wire-overrides-config pattern as `ObjectDetection`'s `class` input.
pub(super) static REGION_IN: [Port; 1] = [p("camera", types::CAMERA, false)];
pub(super) static REGION_OUT: [Port; 1] = [p("region", types::REGION, false)];

pub(super) static BREAK_CAMERA_IN: [Port; 1] = [p("camera", types::CAMERA, true)];
pub(super) static BREAK_CAMERA_OUT: [Port; 3] = [
    p("id", types::TEXT, false),
    p("ip", types::TEXT, false),
    p("name", types::TEXT, false),
];

pub(super) static NO_PORTS: [Port; 0] = [];

// TimeGate exec outputs: a single `out` port (fires when the countdown expires).
pub(super) static EXEC_OUT_TIME_GATE: [Port; 1] = [p("out", types::EXEC, false)];

/// Marks an entry payload as a `DelayTrigger` window expiry — the trailing-edge delivery from
/// the window consumer. The node tests for this rather than trusting `forced` alone, because a
/// loop back-edge can force a node too and must not be mistaken for an expired window.
pub const WINDOW_STOP_MARKER: &str = "__window_stop";
/// Entry-payload key carrying a Device Gate verdict back to its node (#176). Its PRESENCE is what
/// distinguishes an answer-resume from an ordinary force — a loop back-edge forces a node too, and
/// mistaking one for the other would fire an arm nobody chose. Same test the window stop uses, for
/// the same reason.
pub const DEVICE_GATE_VERDICT: &str = "__device_gate_verdict";
/// Which device answered (label) and with which factor, carried alongside the verdict so the
/// node's data outputs can report them without a second query.
pub const DEVICE_GATE_ANSWERED_BY: &str = "__device_gate_answered_by";
pub const DEVICE_GATE_FACTOR: &str = "__device_gate_factor";

// Delay Trigger exec outputs: `start` on the leading edge, `stop` on the trailing one.
pub(super) static EXEC_OUT_DELAY_TRIGGER: [Port; 2] = [
    p("start", types::EXEC, false),
    p("stop", types::EXEC, false),
];

// Universal control-flow ports — every node has an `exec_in` and, by default, an
// `exec_out`. They carry no value; they sequence/gate execution. A node kind may
// override its exec OUTPUTS (e.g. `If` splits into `true`/`false`).
pub const EXEC_IN: Port = p("exec_in", types::EXEC, false);
pub const EXEC_OUT: Port = p("exec_out", types::EXEC, false);
pub const EXEC_TRUE: Port = p("true", types::EXEC, false);
pub const EXEC_FALSE: Port = p("false", types::EXEC, false);
/// `For each` exec outs: `loop_body` fires once per element; `completed` fires once after.
pub const EXEC_LOOP_BODY: Port = p("loop_body", types::EXEC, false);
pub const EXEC_COMPLETED: Port = p("completed", types::EXEC, false);
pub(super) static EXEC_OUT_DEVICE_GATE: [Port; 3] = [
    p("positive", types::EXEC, false),
    p("negative", types::EXEC, false),
    p("failure", types::EXEC, false),
];

pub(super) static EXEC_OUT_DEFAULT: [Port; 1] = [EXEC_OUT];
pub(super) static EXEC_OUT_IF: [Port; 2] = [EXEC_TRUE, EXEC_FALSE];
pub(super) static EXEC_OUT_FOR_EACH: [Port; 2] = [EXEC_LOOP_BODY, EXEC_COMPLETED];

impl NodeKind {
    /// All user-visible node kinds. Reroute is intentionally excluded: it is a
    /// UI-only wire organiser that the engine collapses before execution.
    pub const ALL: [NodeKind; 90] = [
        NodeKind::GetLastAlert,
        NodeKind::GetRecipient,
        NodeKind::CreateAlert,
        NodeKind::DeliverAlert,
        NodeKind::CameraSnapshot,
        NodeKind::GetCurrentFrame,
        NodeKind::GetCameraByName,
        NodeKind::BreakCamera,
        NodeKind::Region,
        NodeKind::GetTrackedObjects,
        NodeKind::BreakTrackedObject,
        NodeKind::DescribeImage,
        NodeKind::NotifyRecipient,
        NodeKind::Format,
        NodeKind::Wait,
        NodeKind::HttpPost,
        NodeKind::If,
        NodeKind::ForEach,
        NodeKind::FlipFlop,
        NodeKind::Compare,
        NodeKind::NumLt,
        NodeKind::NumLte,
        NodeKind::NumGt,
        NodeKind::NumGte,
        NodeKind::NumEq,
        NodeKind::NumNe,
        NodeKind::NumToText,
        NodeKind::BoolToText,
        NodeKind::MotionDetection,
        NodeKind::MotionWatch,
        NodeKind::PersonDetection,
        NodeKind::ObjectDetection,
        NodeKind::BreakPersonDetection,
        NodeKind::BreakDetection,
        NodeKind::BreakFaceDetection,
        NodeKind::RecognizePerson,
        NodeKind::FaceEmbedding,
        NodeKind::RecognizeFace,
        NodeKind::StoreFace,
        NodeKind::RecordPresence,
        NodeKind::EmitEvent,
        NodeKind::OnEvent,
        NodeKind::Print,
        NodeKind::ShowImage,
        NodeKind::Cooldown,
        NodeKind::DrawBbox,
        NodeKind::DrawText,
        NodeKind::BlurOutsideBbox,
        NodeKind::GetImageSize,
        NodeKind::And,
        NodeKind::Or,
        NodeKind::Not,
        NodeKind::TextCompare,
        NodeKind::StringSwitch,
        NodeKind::CameraSwitch,
        NodeKind::AskAi,
        NodeKind::Comment,
        NodeKind::Title,
        NodeKind::Paragraph,
        NodeKind::Markdown,
        NodeKind::Text,
        NodeKind::Gauge,
        NodeKind::StartRecording,
        NodeKind::StopRecording,
        NodeKind::TimeGate,
        NodeKind::DelayTrigger,
        NodeKind::DeviceGate,
        NodeKind::GetVariable,
        NodeKind::SetVariable,
        NodeKind::MultiSetVariable,
        NodeKind::MultiGetVariable,
        NodeKind::ReplyToSender,
        NodeKind::AiAnswer,
        NodeKind::ReplyWithRecording,
        NodeKind::AiSwitch,
        NodeKind::AiExtract,
        NodeKind::DictRowCount,
        NodeKind::DictGetRow,
        NodeKind::DictFindRow,
        NodeKind::DictGetCell,
        NodeKind::DictAddRow,
        NodeKind::DictSetCell,
        NodeKind::DictUpdateRow,
        NodeKind::DictDeleteRow,
        NodeKind::DictClear,
        NodeKind::DictSchema,
        NodeKind::DictMakeRow,
        NodeKind::DictAppendRow,
        NodeKind::TuyaControl,
        NodeKind::TuyaReadStatus,
    ];

    /// The `snake_case` wire name (matches the serde rename) — used by the
    /// frontend palette and `add_node`.
    pub fn slug(self) -> &'static str {
        match self {
            NodeKind::GetLastAlert => "get_last_alert",
            NodeKind::GetRecipient => "get_recipient",
            NodeKind::CreateAlert => "create_alert",
            NodeKind::DeliverAlert => "deliver_alert",
            NodeKind::CameraSnapshot => "camera_snapshot",
            NodeKind::GetTrackedObjects => "get_tracked_objects",
            NodeKind::GetCurrentFrame => "get_current_frame",
            NodeKind::GetCameraByName => "get_camera_by_name",
            NodeKind::BreakCamera => "break_camera",
            NodeKind::Region => "region",
            NodeKind::DescribeImage => "describe_image",
            NodeKind::NotifyRecipient => "notify_recipient",
            NodeKind::Format => "format",
            NodeKind::Wait => "wait",
            NodeKind::HttpPost => "http_post",
            NodeKind::If => "if",
            NodeKind::ForEach => "for_each",
            NodeKind::MotionDetection => "motion_detection",
            NodeKind::MotionWatch => "motion_watch",
            NodeKind::PersonDetection => "person_detection",
            NodeKind::ObjectDetection => "object_detection",
            NodeKind::BreakPersonDetection => "break_person_detection",
            NodeKind::BreakTrackedObject => "break_tracked_object",
            NodeKind::BreakDetection => "break_detection",
            NodeKind::BreakFaceDetection => "break_face_detection",
            NodeKind::RecognizePerson => "recognize_person",
            NodeKind::FaceEmbedding => "face_embedding",
            NodeKind::RecognizeFace => "recognize_face",
            NodeKind::StoreFace => "store_face",
            NodeKind::RecordPresence => "record_presence",
            NodeKind::EmitEvent => "emit_event",
            NodeKind::OnEvent => "on_event",
            NodeKind::Print => "print",
            NodeKind::ShowImage => "show_image",
            NodeKind::FlipFlop => "flip_flop",
            NodeKind::Compare => "compare",
            NodeKind::NumLt => "num_lt",
            NodeKind::NumLte => "num_lte",
            NodeKind::NumGt => "num_gt",
            NodeKind::NumGte => "num_gte",
            NodeKind::NumEq => "num_eq",
            NodeKind::NumNe => "num_ne",
            NodeKind::NumToText => "num_to_text",
            NodeKind::BoolToText => "bool_to_text",
            NodeKind::Cooldown => "cooldown",
            NodeKind::DrawBbox => "draw_bbox",
            NodeKind::DrawText => "draw_text",
            NodeKind::BlurOutsideBbox => "blur_outside_bbox",
            NodeKind::GetImageSize => "get_image_size",
            NodeKind::And => "and",
            NodeKind::Or => "or",
            NodeKind::Not => "not",
            NodeKind::TextCompare => "text_compare",
            NodeKind::StringSwitch => "string_switch",
            NodeKind::CameraSwitch => "camera_switch",
            NodeKind::AskAi => "ask_ai",
            NodeKind::Comment => "comment",
            NodeKind::Title => "title",
            NodeKind::Paragraph => "paragraph",
            NodeKind::Markdown => "markdown",
            NodeKind::Text => "text",
            NodeKind::Gauge => "gauge",
            NodeKind::Reroute => "reroute",
            NodeKind::StartRecording => "start_recording",
            NodeKind::StopRecording => "stop_recording",
            NodeKind::TimeGate => "time_gate",
            NodeKind::DelayTrigger => "delay_trigger",
            NodeKind::DeviceGate => "device_gate",
            NodeKind::GetVariable => "get_variable",
            NodeKind::SetVariable => "set_variable",
            NodeKind::MultiSetVariable => "multi_set_variable",
            NodeKind::MultiGetVariable => "multi_get_variable",
            NodeKind::ReplyToSender => "reply_to_sender",
            NodeKind::AiAnswer => "ai_answer",
            NodeKind::ReplyWithRecording => "reply_with_recording",
            NodeKind::AiSwitch => "ai_switch",
            NodeKind::AiExtract => "ai_extract",
            NodeKind::DictRowCount => "dict_row_count",
            NodeKind::DictGetRow => "dict_get_row",
            NodeKind::DictFindRow => "dict_find_row",
            NodeKind::DictGetCell => "dict_get_cell",
            NodeKind::DictAddRow => "dict_add_row",
            NodeKind::DictSetCell => "dict_set_cell",
            NodeKind::DictUpdateRow => "dict_update_row",
            NodeKind::DictDeleteRow => "dict_delete_row",
            NodeKind::DictClear => "dict_clear",
            NodeKind::DictSchema => "dict_schema",
            NodeKind::DictMakeRow => "dict_make_row",
            NodeKind::DictAppendRow => "dict_append_row",
            NodeKind::TuyaControl => "tuya_control",
            NodeKind::TuyaReadStatus => "tuya_read_status",
        }
    }

    pub fn from_slug(s: &str) -> Option<NodeKind> {
        NodeKind::ALL.into_iter().find(|k| k.slug() == s)
    }

    pub fn label(self) -> &'static str {
        match self {
            NodeKind::GetLastAlert => "Last Alert",
            NodeKind::GetRecipient => "Find Recipient",
            NodeKind::CreateAlert => "Create Alert",
            NodeKind::DeliverAlert => "Send Alert",
            NodeKind::CameraSnapshot => "Snapshot",
            NodeKind::GetTrackedObjects => "Get Tracked Objects",
            NodeKind::GetCurrentFrame => "Current Frame",
            NodeKind::GetCameraByName => "Find Camera",
            NodeKind::BreakCamera => "Unpack Camera",
            NodeKind::Region => "Region",
            NodeKind::DescribeImage => "Describe Image",
            NodeKind::NotifyRecipient => "Notify",
            NodeKind::Format => "Format",
            NodeKind::Wait => "Wait",
            NodeKind::HttpPost => "HTTP Post",
            NodeKind::If => "Branch",
            NodeKind::ForEach => "For Each",
            NodeKind::MotionDetection => "Detect Motion",
            NodeKind::MotionWatch => "Watch for Motion",
            NodeKind::PersonDetection => "Detect People",
            NodeKind::ObjectDetection => "Detect Objects",
            NodeKind::BreakPersonDetection => "Unpack Person",
            NodeKind::BreakTrackedObject => "Unpack Tracked Object",
            NodeKind::BreakDetection => "Unpack Detection",
            NodeKind::BreakFaceDetection => "Unpack Face",
            NodeKind::RecognizePerson => "Recognize Person",
            NodeKind::FaceEmbedding => "Extract Face Embedding",
            NodeKind::RecognizeFace => "Recognize Face",
            NodeKind::StoreFace => "Store Face",
            NodeKind::RecordPresence => "Record Presence",
            NodeKind::EmitEvent => "Send Event",
            NodeKind::OnEvent => "When Event",
            NodeKind::Print => "Print",
            NodeKind::ShowImage => "Show Image",
            NodeKind::FlipFlop => "Toggle",
            NodeKind::Compare => "Compare",
            NodeKind::NumLt => "Less Than",
            NodeKind::NumLte => "Less Than or Equal",
            NodeKind::NumGt => "Greater Than",
            NodeKind::NumGte => "Greater Than or Equal",
            NodeKind::NumEq => "Number Equal",
            NodeKind::NumNe => "Number Not Equal",
            NodeKind::NumToText => "Number to Text",
            NodeKind::BoolToText => "Yes/No to Text",
            NodeKind::Cooldown => "Cooldown",
            NodeKind::DrawBbox => "Draw Detection Box",
            NodeKind::DrawText => "Draw Text",
            NodeKind::BlurOutsideBbox => "Blur Outside Box",
            NodeKind::GetImageSize => "Image Size",
            NodeKind::And => "And",
            NodeKind::Or => "Or",
            NodeKind::Not => "Not",
            NodeKind::TextCompare => "Text Compare",
            NodeKind::StringSwitch => "Text Switch",
            NodeKind::CameraSwitch => "Camera Switch",
            NodeKind::AskAi => "AI Branch",
            NodeKind::Comment => "Comment",
            NodeKind::Title => "Title",
            NodeKind::Paragraph => "Paragraph",
            NodeKind::Markdown => "Markdown",
            NodeKind::Text => "Text",
            NodeKind::Gauge => "Gauge",
            NodeKind::Reroute => "Reroute",
            NodeKind::StartRecording => "Start Recording",
            NodeKind::StopRecording => "Stop Recording",
            NodeKind::TimeGate => "Time Gate",
            NodeKind::DelayTrigger => "Delay Trigger",
            NodeKind::DeviceGate => "Device Gate",
            NodeKind::GetVariable => "Get Variable",
            NodeKind::SetVariable => "Set Variable",
            NodeKind::MultiSetVariable => "Set Variables",
            NodeKind::MultiGetVariable => "Get Variables",
            NodeKind::ReplyToSender => "Reply to Sender",
            NodeKind::AiAnswer => "AI Answer",
            NodeKind::ReplyWithRecording => "Reply with Recording",
            NodeKind::AiSwitch => "AI Switch",
            NodeKind::AiExtract => "AI Extract",
            NodeKind::DictRowCount => "Dictionary Table Row Count",
            NodeKind::DictGetRow => "Dictionary Table Get Row",
            NodeKind::DictFindRow => "Dictionary Table Find Row",
            NodeKind::DictGetCell => "Dictionary Table Get Cell",
            NodeKind::DictAddRow => "Dictionary Table Add Row",
            NodeKind::DictSetCell => "Dictionary Table Set Cell",
            NodeKind::DictUpdateRow => "Dictionary Table Update Row",
            NodeKind::DictDeleteRow => "Dictionary Table Delete Row",
            NodeKind::DictClear => "Dictionary Table Clear",
            NodeKind::DictSchema => "Dictionary Schema",
            NodeKind::DictMakeRow => "Create Dictionary Row",
            NodeKind::DictAppendRow => "Add Dictionary Row",
            NodeKind::TuyaControl => "Control Tuya Device",
            NodeKind::TuyaReadStatus => "Read Tuya Status",
        }
    }

    /// The module this node belongs to — used to group the editor's node palette.
    pub fn category(self) -> &'static str {
        match self {
            NodeKind::GetLastAlert | NodeKind::CreateAlert | NodeKind::DeliverAlert => "Alerts",
            NodeKind::GetRecipient | NodeKind::NotifyRecipient => "Recipients",
            NodeKind::CameraSnapshot
            | NodeKind::GetCurrentFrame
            | NodeKind::GetCameraByName
            | NodeKind::BreakCamera => "Cameras",
            NodeKind::DescribeImage
            | NodeKind::AskAi
            | NodeKind::AiAnswer
            | NodeKind::AiExtract => "AI",
            NodeKind::ReplyToSender | NodeKind::ReplyWithRecording => "Telegram",
            NodeKind::Format => "Text",
            NodeKind::Wait
            | NodeKind::If
            | NodeKind::ForEach
            | NodeKind::FlipFlop
            | NodeKind::Cooldown
            | NodeKind::EmitEvent
            | NodeKind::OnEvent
            | NodeKind::StringSwitch
            | NodeKind::CameraSwitch
            | NodeKind::AiSwitch
            | NodeKind::TimeGate
            | NodeKind::DelayTrigger
            | NodeKind::DeviceGate => "Control",
            NodeKind::NumLt
            | NodeKind::NumLte
            | NodeKind::NumGt
            | NodeKind::NumGte
            | NodeKind::NumEq
            | NodeKind::NumNe
            | NodeKind::NumToText
            | NodeKind::BoolToText
            | NodeKind::Compare
            | NodeKind::And
            | NodeKind::Or
            | NodeKind::Not
            | NodeKind::TextCompare => "Logic",
            NodeKind::HttpPost => "Network",
            NodeKind::MotionDetection
            | NodeKind::MotionWatch
            | NodeKind::PersonDetection
            | NodeKind::ObjectDetection
            | NodeKind::RecognizePerson
            | NodeKind::FaceEmbedding
            | NodeKind::RecognizeFace
            | NodeKind::StoreFace
            | NodeKind::BreakPersonDetection
            | NodeKind::BreakDetection
            | NodeKind::BreakFaceDetection
            | NodeKind::DrawBbox
            | NodeKind::DrawText
            | NodeKind::BlurOutsideBbox
            | NodeKind::GetImageSize
            | NodeKind::Region
            | NodeKind::GetTrackedObjects
            | NodeKind::BreakTrackedObject => "Vision",
            NodeKind::RecordPresence => "Presence",
            NodeKind::Print | NodeKind::ShowImage => "Debug",
            // Comment + Reroute are canvas organizers, not debug sinks.
            NodeKind::Comment | NodeKind::Reroute => "Utility",
            // Presentation nodes that render content on the canvas (docs + live values).
            NodeKind::Title
            | NodeKind::Paragraph
            | NodeKind::Markdown
            | NodeKind::Text
            | NodeKind::Gauge => "Display",
            NodeKind::StartRecording | NodeKind::StopRecording => "Recording",
            NodeKind::GetVariable
            | NodeKind::SetVariable
            | NodeKind::MultiSetVariable
            | NodeKind::MultiGetVariable => "Variables",
            NodeKind::DictRowCount
            | NodeKind::DictGetRow
            | NodeKind::DictFindRow
            | NodeKind::DictGetCell
            | NodeKind::DictAddRow
            | NodeKind::DictSetCell
            | NodeKind::DictUpdateRow
            | NodeKind::DictDeleteRow
            | NodeKind::DictClear
            | NodeKind::DictSchema
            | NodeKind::DictMakeRow
            | NodeKind::DictAppendRow => "Dictionary Tables",
            NodeKind::TuyaControl | NodeKind::TuyaReadStatus => "Smart Home",
        }
    }
}
