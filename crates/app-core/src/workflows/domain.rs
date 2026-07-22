//! Workflows domain — typed ports, nodes, edges, named graphs, persistence.
//! Ported from the ratatui `src/workflows/domain.rs` (spec 013) for the web app;
//! the ratatui editor state (`WfMode`/`WorkflowsState`/viewport) is dropped — the
//! Svelte editor owns view state. Persistence is path-parameterized → app-data
//! `workflows.json`.
//!
//! Isolation model: a node declares typed input/output **ports**; **edges** map a
//! source output port to a target input port. The engine owns all edges and
//! resolves wiring — a node never references another node.

pub use crate::events::AlertLevel;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Ports
// ---------------------------------------------------------------------------

/// The value kinds that flow along edges. An edge is valid only if its source
/// output port and target input port share the same `PortType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PortType {
    Text,
    Num,
    Bool,
    Image,
    /// A notification recipient handle (carried as the recipient's label).
    Recipient,
    /// An arbitrary string→string dictionary (e.g. HTTP headers / form fields).
    Dictionary,
    /// A reference to a named, typed-column **Dictionary table** (`dictionaries` table, #113) —
    /// carried as the dictionary's uuid on the wire, resolved to a `PortValue::Table` at run start.
    /// The structured cousin of `Dictionary`: columns-with-types + data rows, reusable + mutable.
    Table,
    /// Any file (raw bytes + filename), like `Image` but for arbitrary types.
    File,
    /// Control flow ("white") — carries no value; sequences node execution.
    Exec,
    // ---- Typed arrays (iterated by `ForEach`) + their element types ----
    /// `String[]`.
    TextArray,
    /// `Number[]`.
    NumArray,
    /// `PersonDetection[]` (e.g. the Person detection node's `persons` output).
    PersonArray,
    /// `FaceDetection[]`.
    FaceArray,
    /// A single `PersonDetection` (e.g. a `ForEach` element).
    Person,
    /// A single `FaceDetection`.
    Face,
    /// A face **embedding** — a fixed-length float vector (512-d ArcFace, #117). Produced by
    /// `FaceEmbedding`, consumed by `RecognizeFace`/`StoreFace`. A real type so the model/dim is the
    /// producer's concern and embedding wires can't be crossed with a plain `NumArray`.
    Embedding,
    /// A bounding box — top-left/bottom-right pixel rectangle (canonical xyxy). A real type
    /// (not a bare `NumArray`) so producers normalise at the source and consumers never guess
    /// the coordinate convention. Produced by `Break*` nodes + `OnEvent("motion")`; consumed
    /// by `DrawBbox`, `BlurOutsideBbox`, `RecordPresence`.
    Bbox,
    /// The 5 facial landmarks (right/left eye, nose, right/left mouth) for ONE face, in source
    /// pixels. A real type (not a bare `NumArray`) so producers normalise at the source. Produced by
    /// `OnEvent("face_detected")` (YuNet `kps`); consumed by `FaceEmbedding` to align the ArcFace
    /// crop instead of re-detecting (SCRFD) inside it (#122).
    Landmarks,
    /// A camera device reference (stable device id, carried as a string on the wire).
    /// Distinct from `Text` so the editor enforces camera-to-camera wiring.
    Camera,
    /// A reusable named line/zone (card: edge tracking) — 2 points is a crossing line, 3+ is a
    /// polygon region. Produced by `Region`; consumed by `OnEvent`'s `region` input
    /// (`object_entered`/`object_exited`/`line_crossed`), which reads it per its own event kind.
    Region,
    /// One currently-tracked entity, as reported by `GetTrackedObjects` — the "state of the
    /// world" query counterpart to the entered/exited/crossed *events*.
    TrackedObject,
    /// `TrackedObject[]` — `GetTrackedObjects`' main output, iterated by `ForEach`.
    TrackedObjectArray,
    /// Wildcard — compatible with ANY type at wire time. Used by `ForEach` (`items`/`item`)
    /// so one loop node handles every array/element type. See [`Graph::compatible`].
    Any,
}

/// A camera device as it appears on the wire — the safe, credential-free subset of `Device`.
/// Produced by `GetCameraByName` and `OnEvent("motion")`. Consumed by camera-input nodes and
/// `BreakCamera`. The `id` is the stable ONVIF device id (same as `Device::id`).
#[derive(Debug, Clone)]
pub struct CameraEntity {
    pub id: String,
    pub ip: String,
    pub name: String,
}

/// A reusable named line/zone, as it appears on the wire — produced by `Region`. Normalized
/// `0.0..1.0` frame coordinates, resolution-independent (same convention as the edge's
/// `app_core::tracking::Line`/`Polygon`, which this maps directly onto). Whether it means a line
/// or a zone is purely by point count, decided by whichever `OnEvent` reads it: exactly 2 points
/// is a crossing line (`line_crossed`'s `p1`/`p2`); 3+ is a polygon (`object_entered`/
/// `object_exited`'s zone) — a node that gets the wrong shape just doesn't fire, not an error.
#[derive(Debug, Clone)]
pub struct RegionValue {
    pub camera: String,
    pub name: String,
    pub points: Vec<(f32, f32)>,
}

/// One currently-tracked entity as `GetTrackedObjects` reports it — the "state of the world"
/// query counterpart to the `object_entered`/`object_exited`/`line_crossed` *events*. `zone` is
/// the name of the region it's currently inside, empty if none (either it's not in any configured
/// zone, or its job doesn't have one).
#[derive(Debug, Clone)]
pub struct TrackedObjectInfo {
    pub uuid: String,
    pub class: String,
    pub bbox: BBox,
    pub zone: String,
}

/// A bounding box in **canonical xyxy pixel coordinates**: top-left `(x1,y1)` → bottom-right
/// `(x2,y2)`. One convention everywhere — producers convert at the source (`from_xywh` for the
/// edge's motion blobs, identity for detector xyxy) so consumers (`DrawBbox`, `BlurOutsideBbox`,
/// `RecordPresence`) never need a `format` toggle. Serialised as a 4-element array `[x1,y1,x2,y2]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BBox {
    pub x1: i64,
    pub y1: i64,
    pub x2: i64,
    pub y2: i64,
}

impl BBox {
    pub fn from_xyxy(x1: i64, y1: i64, x2: i64, y2: i64) -> Self {
        Self { x1, y1, x2, y2 }
    }
    pub fn from_xywh(x: i64, y: i64, w: i64, h: i64) -> Self {
        Self {
            x1: x,
            y1: y,
            x2: x + w,
            y2: y + h,
        }
    }
    /// Parse 4 numbers (xyxy) — returns `None` if fewer than 4 are present.
    pub fn from_xyxy_slice(v: &[i64]) -> Option<Self> {
        if v.len() >= 4 {
            Some(Self {
                x1: v[0],
                y1: v[1],
                x2: v[2],
                y2: v[3],
            })
        } else {
            None
        }
    }
    pub fn to_array(self) -> [i64; 4] {
        [self.x1, self.y1, self.x2, self.y2]
    }
}

/// A static port declaration on a `NodeKind` (not persisted — derived from the kind).
#[derive(Debug, Clone, Copy)]
pub struct Port {
    pub name: &'static str,
    pub ty: PortType,
    /// Inputs only: a required input with no edge and no config literal fails the run.
    pub required: bool,
}

const fn p(name: &'static str, ty: PortType, required: bool) -> Port {
    Port { name, ty, required }
}

/// An owned port declaration — like [`Port`] but with an owned name, for ports that are
/// **derived from a node's config** rather than static on the kind (today: the payload
/// ports of `EmitEvent`/`OnEvent`, one per field of the configured event). See
/// [`GraphNode::input_ports`].
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedPort {
    pub name: String,
    pub ty: PortType,
    pub required: bool,
}

impl From<&Port> for ResolvedPort {
    fn from(p: &Port) -> Self {
        ResolvedPort {
            name: p.name.to_string(),
            ty: p.ty,
            required: p.required,
        }
    }
}

/// The payload ports baked into an `EmitEvent`/`OnEvent` node's config (`config.payload`
/// is `[{ name, type }]`, copied from the event's registry schema at config time). Empty
/// when the event is payload-less or unset. Used as inputs on `EmitEvent`, outputs on
/// `OnEvent`.
fn payload_ports(config: &Value) -> Vec<ResolvedPort> {
    config
        .get("payload")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|f| {
                    let name = f.get("name").and_then(|v| v.as_str())?.trim().to_string();
                    if name.is_empty() {
                        return None;
                    }
                    let ty = f
                        .get("type")
                        .and_then(|v| serde_json::from_value::<PortType>(v.clone()).ok())?;
                    Some(ResolvedPort {
                        name,
                        ty,
                        required: false,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Map a Dictionary column's cell-type string (`text`/`num`/`bool`/`camera`/`recipient`) to the
/// `PortType` a `DictMakeRow` column input carries. Unknown → `Text`.
fn cell_type_to_port(ty: &str) -> PortType {
    match ty {
        "num" => PortType::Num,
        "bool" => PortType::Bool,
        "camera" => PortType::Camera,
        "recipient" => PortType::Recipient,
        _ => PortType::Text,
    }
}

/// The config-derived input ports of a `DictMakeRow`: the fixed `schema` (a Table, required —
/// structural/UI dependency) plus one optional input per configured column, typed by the column's
/// cell type. Mirrors [`payload_ports`]: the runtime column list is baked into `config.columns`.
fn dict_row_ports(config: &Value) -> Vec<ResolvedPort> {
    let mut ports = vec![ResolvedPort {
        name: "schema".to_string(),
        ty: PortType::Table,
        required: true,
    }];
    if let Some(cols) = config.get("columns").and_then(|v| v.as_array()) {
        for c in cols {
            let name = match c.get("name").and_then(|v| v.as_str()) {
                Some(n) if !n.trim().is_empty() => n.trim().to_string(),
                _ => continue,
            };
            let ty = c.get("type").and_then(|v| v.as_str()).unwrap_or("text");
            ports.push(ResolvedPort {
                name,
                ty: cell_type_to_port(ty),
                required: false,
            });
        }
    }
    ports
}

/// The single typed port of a Get/Set Variable node — derived from `config.type` (the
/// variable's declared wire type, baked into the node at create time, same as `payload_ports`
/// bakes event fields). `required` is set true for the Set value input, false for the Get
/// output. Defaults to `Text` when the type is unset/unknown.
fn variable_port(config: &Value, required: bool) -> Vec<ResolvedPort> {
    let ty = config
        .get("type")
        .and_then(|v| serde_json::from_value::<PortType>(v.clone()).ok())
        .unwrap_or(PortType::Text);
    vec![ResolvedPort {
        name: "value".to_string(),
        ty,
        required,
    }]
}

/// `TuyaControl`'s `value` input port, typed by the selected data point (baked into `value_type`
/// by the editor when a `tuya_code` is chosen: `bool`/`num`/`text`). Wireable so a dynamic value
/// can drive the device; when unwired the node falls back to the config literal. Mirrors
/// `variable_port` (Set Variable). Optional — a config literal covers the common static case.
fn tuya_value_port(config: &Value) -> Vec<ResolvedPort> {
    let ty = config
        .get("value_type")
        .and_then(|v| serde_json::from_value::<PortType>(v.clone()).ok())
        .unwrap_or(PortType::Bool);
    vec![ResolvedPort { name: "value".to_string(), ty, required: false }]
}

/// `TuyaReadStatus`'s outputs: `value` typed by the selected data point (`value_type`, like the
/// control node's input) + the static `on` (Bool convenience) + `raw` (full status JSON). So the
/// read value wires straight into the right type (e.g. `countdown_1` → Num).
fn tuya_read_output_ports(config: &Value) -> Vec<ResolvedPort> {
    let ty = config
        .get("value_type")
        .and_then(|v| serde_json::from_value::<PortType>(v.clone()).ok())
        .unwrap_or(PortType::Text);
    vec![
        ResolvedPort { name: "value".to_string(), ty, required: false },
        ResolvedPort { name: "on".to_string(), ty: PortType::Bool, required: false },
        ResolvedPort { name: "raw".to_string(), ty: PortType::Text, required: false },
    ]
}

/// `OnEvent`'s extra, config-derived input ports: `camera` + `region`, wireable (from
/// `GetCameraByName` / `Region`) — present only when the configured event is one of the edge
/// tracker's three. These GATE delivery (see `engine::on_event_node`), not just display a value:
/// a scoped `OnEvent` only fires for a matching camera/region, an unscoped one fires for any.
/// This is the general mechanism for "events with inputs that filter the trigger" — extend this
/// list (and the matching gate in `on_event_node`) rather than inventing a new node kind for the
/// next event that needs scoping (see `docs/ref/workflows.md`).
fn on_event_input_ports(config: &Value) -> Vec<ResolvedPort> {
    let event = config.get("event").and_then(|v| v.as_str()).unwrap_or("");
    if matches!(event, "object_entered" | "object_exited" | "line_crossed") {
        vec![
            ResolvedPort { name: "camera".to_string(), ty: PortType::Camera, required: false },
            ResolvedPort { name: "region".to_string(), ty: PortType::Region, required: false },
        ]
    } else {
        Vec::new()
    }
}

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
    /// `completed` once after the last. Body nodes should be memoize=off so they re-run.
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
    /// Variables: read a workflow variable's current value → one typed data output (`value`).
    /// Pure (no exec pins) — drop it anywhere and wire its output, no flow wires needed. The
    /// variable name + type are in config; the output port type follows the variable's type.
    GetVariable,
    /// Variables: write a value into a workflow variable. One typed data input (`value`) + an
    /// exec passthrough so it writes at an exact point in the flow. The variable name + type
    /// are in config; the input port type follows the variable's type.
    SetVariable,
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
const GET_LAST_ALERT_OUT: [Port; 7] = [
    p("id", PortType::Num, false),
    p("level", PortType::Text, false),
    p("message", PortType::Text, false),
    p("room", PortType::Text, false),
    p("source", PortType::Text, false),
    p("created_at", PortType::Text, false),
    p("ok", PortType::Bool, false),
];

const CREATE_ALERT_IN: [Port; 2] = [
    p("message", PortType::Text, true),
    p("room", PortType::Text, false),
];
const CREATE_ALERT_OUT: [Port; 3] = [
    p("id", PortType::Num, false),
    p("result", PortType::Text, false),
    p("ok", PortType::Bool, false),
];

const RECORD_PRESENCE_IN: [Port; 8] = [
    p("room", PortType::Text, false),
    p("category", PortType::Text, false),
    p("label", PortType::Text, false),
    p("camera", PortType::Camera, false),
    p("bbox", PortType::Bbox, false), // [x1,y1,x2,y2] from BreakPersonDetection
    p("frame_w", PortType::Num, false), // source image width in pixels
    p("frame_h", PortType::Num, false), // source image height in pixels
    p("duration_secs", PortType::Num, false),
];
const RECORD_PRESENCE_OUT: [Port; 2] = [
    p("id", PortType::Num, false),
    p("ok", PortType::Bool, false),
];

const DELIVER_ALERT_IN: [Port; 1] = [p("id", PortType::Num, true)];
const DELIVER_ALERT_OUT: [Port; 2] = [
    p("result", PortType::Text, false),
    p("ok", PortType::Bool, false),
];

const CAMERA_SNAPSHOT_IN: [Port; 1] = [p("camera", PortType::Camera, true)];
const CAMERA_SNAPSHOT_OUT: [Port; 5] = [
    p("image", PortType::Image, false),
    p("result", PortType::Text, false),
    p("width", PortType::Num, false),
    p("height", PortType::Num, false),
    p("ok", PortType::Bool, false),
];

const GET_TRACKED_OBJECTS_IN: [Port; 1] = [p("camera", PortType::Camera, true)];
const GET_TRACKED_OBJECTS_OUT: [Port; 2] = [
    p("objects", PortType::TrackedObjectArray, false),
    p("count", PortType::Num, false),
];

const GET_CURRENT_FRAME_IN: [Port; 1] = [p("camera", PortType::Camera, true)];
const GET_CURRENT_FRAME_OUT: [Port; 5] = [
    p("image", PortType::Image, false),
    p("age_ms", PortType::Num, false), // how old the returned frame is (0 = fresh capture)
    p("width", PortType::Num, false),
    p("height", PortType::Num, false),
    p("ok", PortType::Bool, false),
];

const DESCRIBE_IMAGE_IN: [Port; 2] = [
    p("image", PortType::Image, true),
    // Optional guiding question — when wired/set, the caption answers it about the frame.
    p("question", PortType::Text, false),
];
const DESCRIBE_IMAGE_OUT: [Port; 2] = [
    p("description", PortType::Text, false),
    p("ok", PortType::Bool, false),
];

const GET_RECIPIENT_OUT: [Port; 2] = [
    p("recipient", PortType::Recipient, false),
    p("ok", PortType::Bool, false),
];

const NOTIFY_RECIPIENT_IN: [Port; 3] = [
    p("message", PortType::Text, true),
    p("recipient", PortType::Recipient, false),
    p("image", PortType::Image, false),
];
const NOTIFY_RECIPIENT_OUT: [Port; 2] = [
    p("result", PortType::Text, false),
    p("ok", PortType::Bool, false),
];

const FORMAT_IN: [Port; 3] = [
    p("p1", PortType::Text, false),
    p("p2", PortType::Text, false),
    p("p3", PortType::Text, false),
];
const FORMAT_OUT: [Port; 1] = [p("text", PortType::Text, false)];

// HTTP POST: url + arbitrary header/field maps + an optional file (→ multipart).
const HTTP_POST_IN: [Port; 4] = [
    p("url", PortType::Text, true),
    p("headers", PortType::Dictionary, false),
    p("fields", PortType::Dictionary, false),
    p("file", PortType::File, false),
];
const HTTP_POST_OUT: [Port; 3] = [
    p("status", PortType::Num, false),
    p("response", PortType::Text, false),
    p("ok", PortType::Bool, false),
];

// Smart home (Tuya): device + data point are config dropdowns (no data inputs), like the
// camera picker on `GetCameraByName`.
const TUYA_CONTROL_OUT: [Port; 2] = [
    p("ok", PortType::Bool, false),
    p("response", PortType::Text, false),
];
const TUYA_READ_STATUS_OUT: [Port; 3] = [
    p("value", PortType::Text, false),
    p("on", PortType::Bool, false),
    p("raw", PortType::Text, false),
];

// ---- Dictionaries (#113) ----
// A `row` is emitted / consumed as a colname→value string Dictionary (consumable by Format,
// HTTP Post fields, etc.). The `table` port is a reference to a named dictionary (dropdown-picked
// id, or wired from a Table variable).
const DICT_ROW_COUNT_IN: [Port; 1] = [p("table", PortType::Table, true)];
const DICT_ROW_COUNT_OUT: [Port; 1] = [p("count", PortType::Num, false)];

const DICT_GET_ROW_IN: [Port; 2] = [
    p("table", PortType::Table, true),
    p("index", PortType::Num, true),
];
const DICT_GET_ROW_OUT: [Port; 2] = [
    p("row", PortType::Dictionary, false),
    p("found", PortType::Bool, false),
];

const DICT_FIND_ROW_IN: [Port; 3] = [
    p("table", PortType::Table, true),
    p("column", PortType::Text, true),
    p("key", PortType::Text, true),
];
const DICT_FIND_ROW_OUT: [Port; 3] = [
    p("row", PortType::Dictionary, false),
    p("index", PortType::Num, false),
    p("found", PortType::Bool, false),
];

const DICT_GET_CELL_IN: [Port; 3] = [
    p("table", PortType::Table, true),
    p("row", PortType::Num, true),
    p("column", PortType::Text, true),
];
const DICT_GET_CELL_OUT: [Port; 2] = [
    p("value", PortType::Text, false),
    p("found", PortType::Bool, false),
];

const DICT_ADD_ROW_IN: [Port; 2] = [
    p("table", PortType::Table, true),
    p("values", PortType::Dictionary, false),
];
const DICT_SET_CELL_IN: [Port; 4] = [
    p("table", PortType::Table, true),
    p("row", PortType::Num, true),
    p("column", PortType::Text, true),
    p("value", PortType::Text, false),
];
const DICT_UPDATE_ROW_IN: [Port; 4] = [
    p("table", PortType::Table, true),
    p("column", PortType::Text, true),
    p("key", PortType::Text, true),
    p("values", PortType::Dictionary, false),
];
const DICT_DELETE_ROW_IN: [Port; 3] = [
    p("table", PortType::Table, true),
    p("column", PortType::Text, true),
    p("key", PortType::Text, true),
];
const DICT_CLEAR_IN: [Port; 1] = [p("table", PortType::Table, true)];

// Build nodes (compose a table in-graph, no DB). DictSchema seeds an empty typed table from its
// config columns; DictMakeRow's per-column inputs are dynamic (see `dict_row_ports`) — this static
// list is the fallback (just `schema`); DictAppendRow appends a `row` Dictionary to a `table`.
const DICT_SCHEMA_OUT: [Port; 1] = [p("schema", PortType::Table, false)];
const DICT_MAKE_ROW_IN: [Port; 1] = [p("schema", PortType::Table, true)];
const DICT_MAKE_ROW_OUT: [Port; 1] = [p("row", PortType::Dictionary, false)];
const DICT_APPEND_ROW_IN: [Port; 2] = [
    p("table", PortType::Table, true),
    p("row", PortType::Dictionary, true),
];
const DICT_APPEND_ROW_OUT: [Port; 1] = [p("table", PortType::Table, false)];

// If: a Bool condition routes the run down one of two exec branches.
const IF_IN: [Port; 1] = [p("condition", PortType::Bool, true)];

// For each: iterate ANY array (a wired `*Array`, or a text list authored in config).
// `items`/`item` are `Any` so one loop node handles every element type.
const FOR_EACH_IN: [Port; 1] = [p("items", PortType::Any, false)];
const FOR_EACH_OUT: [Port; 2] = [
    p("item", PortType::Any, false),
    p("index", PortType::Num, false),
];

// Object detection: detect any COCO class — class chosen by config or wired `class` input.
const OBJECT_DETECTION_IN: [Port; 2] = [
    p("image", PortType::Image, true),
    p("class", PortType::Text, false),
];
const OBJECT_DETECTION_OUT: [Port; 6] = [
    p("count", PortType::Num, false),
    p("confidence", PortType::Num, false), // percent 0..100 (top detection)
    p("detections", PortType::PersonArray, false), // one Detection per match — loop this
    p("width", PortType::Num, false),
    p("height", PortType::Num, false),
    p("ok", PortType::Bool, false),
];

// Break (destructure) nodes — split a struct element into its fields, like Unreal's Break.
const BREAK_PERSON_IN: [Port; 1] = [p("person", PortType::Person, true)];
const BREAK_PERSON_OUT: [Port; 2] = [
    p("bbox", PortType::Bbox, false),
    p("confidence", PortType::Num, false),
];
const BREAK_TRACKED_OBJECT_IN: [Port; 1] = [p("object", PortType::TrackedObject, true)];
const BREAK_TRACKED_OBJECT_OUT: [Port; 4] = [
    p("uuid", PortType::Text, false),
    p("class", PortType::Text, false),
    p("bbox", PortType::Bbox, false),
    p("zone", PortType::Text, false),
];
const BREAK_DETECTION_IN: [Port; 1] = [p("detection", PortType::Person, true)];
const BREAK_DETECTION_OUT: [Port; 2] = [
    p("bbox", PortType::Bbox, false),
    p("confidence", PortType::Num, false),
];
const BREAK_FACE_IN: [Port; 1] = [p("face", PortType::Face, true)];
const BREAK_FACE_OUT: [Port; 3] = [
    p("bbox", PortType::Bbox, false),
    p("confidence", PortType::Num, false),
    p("landmarks", PortType::Num, false), // count of the 5-point landmark set
];

// Motion detection: compare two frames; report whether/how much they differ.
const MOTION_IN: [Port; 2] = [
    p("image_a", PortType::Image, true),
    p("image_b", PortType::Image, true),
];
const MOTION_OUT: [Port; 3] = [
    p("moved", PortType::Bool, false),
    p("score", PortType::Num, false),
    p("ok", PortType::Bool, false),
];
const MOTION_WATCH_IN: [Port; 1] = [p("camera", PortType::Camera, true)];

// Person detection: an image in → whether a person is present (via the AI sidecar).
const PERSON_DETECTION_IN: [Port; 1] = [p("image", PortType::Image, true)];
const PERSON_DETECTION_OUT: [Port; 6] = [
    p("count", PortType::Num, false),
    p("confidence", PortType::Num, false), // percent 0..100 (top person)
    p("persons", PortType::PersonArray, false), // one PersonDetection per person — loop this
    p("width", PortType::Num, false),
    p("height", PortType::Num, false),
    p("ok", PortType::Bool, false),
];

// Recognize person: a face image in → the matched person + confidence.
const RECOGNIZE_PERSON_IN: [Port; 1] = [p("image", PortType::Image, true)];
const RECOGNIZE_PERSON_OUT: [Port; 4] = [
    p("person", PortType::Text, false),    // matched person's name
    p("person_id", PortType::Text, false), // matched person's id
    p("confidence", PortType::Num, false), // percent 0..100
    p("ok", PortType::Bool, false),        // true when a match within threshold
];

// ---- Face recognition pipeline (#117) ----
const FACE_EMBEDDING_IN: [Port; 2] = [
    p("image", PortType::Image, true),
    p("bbox", PortType::Bbox, false), // optional crop-to-face (edge already found it)
];
const FACE_EMBEDDING_OUT: [Port; 3] = [
    p("embedding", PortType::Embedding, false),
    p("dim", PortType::Num, false),
    p("ok", PortType::Bool, false),
];
const RECOGNIZE_FACE_IN: [Port; 1] = [p("embedding", PortType::Embedding, true)];
const RECOGNIZE_FACE_OUT: [Port; 5] = [
    p("person", PortType::Text, false),        // best match's name ("" if none / unknown)
    p("person_id", PortType::Text, false),     // best match's person id (wire → StoreFace.label)
    p("confidence", PortType::Num, false),     // percent 0..100 of the best match
    p("matches", PortType::Dictionary, false), // top-K: person/label → score
    p("found", PortType::Bool, false),         // a match within max_distance
];
const STORE_FACE_IN: [Port; 7] = [
    p("image", PortType::Image, false),
    p("embedding", PortType::Embedding, true),
    p("bbox", PortType::Bbox, false),
    p("label", PortType::Text, false),  // a person id; empty = unknown
    p("camera", PortType::Camera, false),
    p("confidence", PortType::Num, false), // detection score 0..100 (from face_detected)
    p("face_ratio", PortType::Num, false), // face area ÷ frame area, 0..100 (far-face signal)
];
const STORE_FACE_OUT: [Port; 2] = [
    p("stored", PortType::Bool, false), // false if skipped by dedup
    p("face_id", PortType::Text, false),
];

// Print: a message to echo to the Console (+ optional on-screen flash).
const PRINT_IN: [Port; 2] = [
    p("message", PortType::Text, false),
    p("on_screen", PortType::Bool, false),
];

// Show image: a debug sink — an image in, flashed on the canvas. No data out.
const SHOW_IMAGE_IN: [Port; 1] = [p("image", PortType::Image, true)];

// Flip Flop: exec a/b outputs (alternating), plus is_a bool data output.
const EXEC_OUT_FLIP_FLOP: [Port; 2] =
    [p("a", PortType::Exec, false), p("b", PortType::Exec, false)];
const FLIP_FLOP_OUT: [Port; 1] = [p("is_a", PortType::Bool, false)];

// Compare: two numeric inputs (wired or literal) → a bool result.
const COMPARE_IN: [Port; 2] = [p("a", PortType::Num, false), p("b", PortType::Num, false)];
const COMPARE_OUT: [Port; 1] = [p("result", PortType::Bool, false)];

// Type cast nodes.
const NUM_TO_TEXT_IN: [Port; 1] = [p("value", PortType::Num, true)];
const BOOL_TO_TEXT_IN: [Port; 1] = [p("value", PortType::Bool, true)];
const CAST_TEXT_OUT: [Port; 1] = [p("text", PortType::Text, false)];

const COOLDOWN_OUT: [Port; 2] = [
    p("elapsed_secs", PortType::Num, false),
    p("ok", PortType::Bool, false),
];

const DRAW_BBOX_IN: [Port; 2] = [
    p("image", PortType::Image, true),
    p("bbox", PortType::Bbox, true),
];
// `bbox` is the box actually drawn — i.e. the input box after the `expand` config is applied,
// so downstream nodes (blur, crop, presence) can reuse the expanded region.
const DRAW_BBOX_OUT: [Port; 3] = [
    p("image", PortType::Image, false),
    p("bbox", PortType::Bbox, false),
    p("ok", PortType::Bool, false),
];

// DrawText: an image + a bbox to anchor against + the text (wired or config literal).
const DRAW_TEXT_IN: [Port; 3] = [
    p("image", PortType::Image, true),
    p("bbox", PortType::Bbox, true),
    p("text", PortType::Text, false),
];
const DRAW_TEXT_OUT: [Port; 2] = [
    p("image", PortType::Image, false),
    p("ok", PortType::Bool, false),
];

const BLUR_OUTSIDE_BBOX_IN: [Port; 2] = [
    p("image", PortType::Image, true),
    p("bbox", PortType::Bbox, true),
];
const BLUR_OUTSIDE_BBOX_OUT: [Port; 2] = [
    p("image", PortType::Image, false),
    p("ok", PortType::Bool, false),
];

const GET_IMAGE_SIZE_IN: [Port; 1] = [p("image", PortType::Image, true)];
const GET_IMAGE_SIZE_OUT: [Port; 3] = [
    p("width", PortType::Num, false),
    p("height", PortType::Num, false),
    p("ok", PortType::Bool, false),
];

const BOOL_OP_IN: [Port; 2] = [p("a", PortType::Bool, false), p("b", PortType::Bool, false)];
const NOT_IN: [Port; 1] = [p("a", PortType::Bool, false)];
const TEXT_COMPARE_IN: [Port; 2] = [p("a", PortType::Text, false), p("b", PortType::Text, false)];

// String switch: one required text input (the value to match against the arms).
const STRING_SWITCH_IN: [Port; 1] = [p("value", PortType::Text, true)];

// Camera switch: one required camera input (the entity to match against the arms).
const CAMERA_SWITCH_IN: [Port; 1] = [p("camera", PortType::Camera, true)];

// AI switch: one required text input (the message to route by meaning across the arms).
const AI_SWITCH_IN: [Port; 1] = [p("text", PortType::Text, true)];

// AI extract: one required text input (the message to pull a value from).
const AI_EXTRACT_IN: [Port; 1] = [p("text", PortType::Text, true)];
// Static template outputs — the real `value` type is dynamic per config.type (GraphNode::output_ports).
const AI_EXTRACT_OUT: [Port; 2] = [
    p("value", PortType::Text, false),
    p("found", PortType::Bool, false),
];

// AI If: optional image + question text. Both are optional at the port level so a
// text-only question (no image wired) works. The node errors at runtime if no question.
const ASK_AI_IN: [Port; 2] = [
    p("image", PortType::Image, false),
    p("question", PortType::Text, false),
];

// Reroute: wildcard passthrough — one Any input, one Any output.
const REROUTE_IN: [Port; 1] = [p("value", PortType::Any, false)];
const REROUTE_OUT: [Port; 1] = [p("value", PortType::Any, false)];

// StartRecording: camera entity + optional pre-roll hint.
const START_RECORDING_IN: [Port; 2] = [
    p("camera", PortType::Camera, true),
    p("pre_roll_secs", PortType::Num, false),
];
const START_RECORDING_OUT: [Port; 1] = [p("clip_id", PortType::Text, false)];

// StopRecording: camera entity + optional clip_id from StartRecording.
const STOP_RECORDING_IN: [Port; 2] = [
    p("camera", PortType::Camera, true),
    p("clip_id", PortType::Text, false),
];

// Reply to sender (Telegram): text reply to the triggering chat + optional image.
const REPLY_TO_SENDER_IN: [Port; 3] = [
    p("message", PortType::Text, true),
    p("chat_id", PortType::Num, true),
    p("image", PortType::Image, false),
];
const REPLY_TO_SENDER_OUT: [Port; 2] = [
    p("queued", PortType::Bool, false),
    p("result", PortType::Text, false),
];

// AI Answer: free-text prompt (required) + optional image → a text response.
const AI_ANSWER_IN: [Port; 2] = [
    p("prompt", PortType::Text, true),
    p("image", PortType::Image, false),
];
const AI_ANSWER_OUT: [Port; 2] = [
    p("response", PortType::Text, false),
    p("ok", PortType::Bool, false),
];

// Reply with recording (Telegram): a clip_id from StartRecording + chat_id + optional caption.
const REPLY_RECORDING_IN: [Port; 3] = [
    p("clip_id", PortType::Text, true),
    p("chat_id", PortType::Num, true),
    p("caption", PortType::Text, false),
];
const REPLY_RECORDING_OUT: [Port; 2] = [
    p("queued", PortType::Bool, false),
    p("result", PortType::Text, false),
];

const GET_CAMERA_BY_NAME_OUT: [Port; 1] = [p("camera", PortType::Camera, false)];
/// `Region`'s `camera` input is optional/wireable (from `GetCameraByName`) with a config-picker
/// fallback — same wire-overrides-config pattern as `ObjectDetection`'s `class` input.
const REGION_IN: [Port; 1] = [p("camera", PortType::Camera, false)];
const REGION_OUT: [Port; 1] = [p("region", PortType::Region, false)];

const BREAK_CAMERA_IN: [Port; 1] = [p("camera", PortType::Camera, true)];
const BREAK_CAMERA_OUT: [Port; 3] = [
    p("id", PortType::Text, false),
    p("ip", PortType::Text, false),
    p("name", PortType::Text, false),
];

const NO_PORTS: [Port; 0] = [];

// TimeGate exec outputs: a single `out` port (fires when the countdown expires).
const EXEC_OUT_TIME_GATE: [Port; 1] = [p("out", PortType::Exec, false)];

/// Marks an entry payload as a `DelayTrigger` window expiry — the trailing-edge delivery from
/// the window consumer. The node tests for this rather than trusting `forced` alone, because a
/// loop back-edge can force a node too and must not be mistaken for an expired window.
pub const WINDOW_STOP_MARKER: &str = "__window_stop";

// Delay Trigger exec outputs: `start` on the leading edge, `stop` on the trailing one.
const EXEC_OUT_DELAY_TRIGGER: [Port; 2] = [
    p("start", PortType::Exec, false),
    p("stop", PortType::Exec, false),
];

// Universal control-flow ports — every node has an `exec_in` and, by default, an
// `exec_out`. They carry no value; they sequence/gate execution. A node kind may
// override its exec OUTPUTS (e.g. `If` splits into `true`/`false`).
pub const EXEC_IN: Port = p("exec_in", PortType::Exec, false);
pub const EXEC_OUT: Port = p("exec_out", PortType::Exec, false);
pub const EXEC_TRUE: Port = p("true", PortType::Exec, false);
pub const EXEC_FALSE: Port = p("false", PortType::Exec, false);
/// `For each` exec outs: `loop_body` fires once per element; `completed` fires once after.
pub const EXEC_LOOP_BODY: Port = p("loop_body", PortType::Exec, false);
pub const EXEC_COMPLETED: Port = p("completed", PortType::Exec, false);
const EXEC_OUT_DEFAULT: [Port; 1] = [EXEC_OUT];
const EXEC_OUT_IF: [Port; 2] = [EXEC_TRUE, EXEC_FALSE];
const EXEC_OUT_FOR_EACH: [Port; 2] = [EXEC_LOOP_BODY, EXEC_COMPLETED];

impl NodeKind {
    /// All user-visible node kinds. Reroute is intentionally excluded: it is a
    /// UI-only wire organiser that the engine collapses before execution.
    pub const ALL: [NodeKind; 82] = [
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
        NodeKind::StartRecording,
        NodeKind::StopRecording,
        NodeKind::TimeGate,
        NodeKind::DelayTrigger,
        NodeKind::GetVariable,
        NodeKind::SetVariable,
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
            NodeKind::Reroute => "reroute",
            NodeKind::StartRecording => "start_recording",
            NodeKind::StopRecording => "stop_recording",
            NodeKind::TimeGate => "time_gate",
            NodeKind::DelayTrigger => "delay_trigger",
            NodeKind::GetVariable => "get_variable",
            NodeKind::SetVariable => "set_variable",
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
            NodeKind::Reroute => "Reroute",
            NodeKind::StartRecording => "Start Recording",
            NodeKind::StopRecording => "Stop Recording",
            NodeKind::TimeGate => "Time Gate",
            NodeKind::DelayTrigger => "Delay Trigger",
            NodeKind::GetVariable => "Get Variable",
            NodeKind::SetVariable => "Set Variable",
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
            NodeKind::DescribeImage | NodeKind::AskAi | NodeKind::AiAnswer | NodeKind::AiExtract => "AI",
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
            | NodeKind::DelayTrigger => "Control",
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
            NodeKind::StartRecording | NodeKind::StopRecording => "Recording",
            NodeKind::GetVariable | NodeKind::SetVariable => "Variables",
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

    /// Pure data/logic nodes have no exec pins — they activate whenever their upstream
    /// data is ready, like pure functions in Unreal Blueprint. Comparison operators,
    /// logical operators, and type-conversion nodes are all pure.
    pub fn is_pure(self) -> bool {
        matches!(
            self,
            NodeKind::GetCameraByName
                | NodeKind::BreakCamera
                | NodeKind::Region
                | NodeKind::Compare
                | NodeKind::NumLt
                | NodeKind::NumLte
                | NodeKind::NumGt
                | NodeKind::NumGte
                | NodeKind::NumEq
                | NodeKind::NumNe
                | NodeKind::NumToText
                | NodeKind::BoolToText
                | NodeKind::And
                | NodeKind::Or
                | NodeKind::Not
                | NodeKind::TextCompare
                | NodeKind::Reroute
                | NodeKind::GetVariable
                | NodeKind::DictRowCount
                | NodeKind::DictGetRow
                | NodeKind::DictFindRow
                | NodeKind::DictGetCell
                | NodeKind::DictSchema
                | NodeKind::DictMakeRow
                | NodeKind::DictAppendRow
        )
    }

    /// Exec OUTPUT pins for this kind. Pure nodes have none; `If` splits into
    /// `true`/`false` branches; `StringSwitch` has user-defined arms (dynamic, see
    /// [`GraphNode::output_port`]); everything else gets a single `exec_out`.
    pub fn exec_out_ports(self) -> &'static [Port] {
        if self.is_pure() {
            return &NO_PORTS;
        }
        match self {
            NodeKind::If | NodeKind::AskAi => &EXEC_OUT_IF,
            NodeKind::ForEach => &EXEC_OUT_FOR_EACH,
            NodeKind::FlipFlop => &EXEC_OUT_FLIP_FLOP,
            // Dynamic: arms come from config, not a static slice. See GraphNode::output_port.
            NodeKind::StringSwitch | NodeKind::CameraSwitch | NodeKind::AiSwitch => &NO_PORTS,
            NodeKind::Comment => &NO_PORTS,
            NodeKind::Reroute => &NO_PORTS, // pure passthrough, no exec pins
            NodeKind::TimeGate => &EXEC_OUT_TIME_GATE, // "out" fires when countdown expires
            NodeKind::DelayTrigger => &EXEC_OUT_DELAY_TRIGGER, // leading "start", trailing "stop"
            _ => &EXEC_OUT_DEFAULT,
        }
    }

    /// A value source with **no side effects** whose result depends only on its config plus a
    /// cheap in-memory lookup — safe to evaluate on any run, in any epoch.
    ///
    /// These activate unconditionally, because the alternative silently breaks them. A source with
    /// no incoming edges is a "true orphan", and orphans fall back to `seed_entries` — which is
    /// **false on every sub-run** (`run_subpath_eventful` passes it so). An orphan source therefore
    /// works on a cron/manual run and is *inert* inside any `OnEvent` or timer sub-path, taking its
    /// consumers down with it: a required input wired to a node that never ran makes the engine
    /// mark the whole branch dead, and skip it without an error. `GetVariable` was exempted by hand
    /// when someone hit exactly this; the others were left broken. This is that exemption,
    /// generalised — so the next source node doesn't have to rediscover it.
    ///
    /// Deliberately NOT included:
    /// - `GetLastAlert` — a database query. Re-running it per epoch isn't free, and whether "last"
    ///   should re-read mid-run is a real question, not an oversight.
    /// - Anything with data inputs (`Region` takes a camera): activating before its producers ran
    ///   would hand it empty inputs, trading a dead branch for a wrong answer.
    pub fn is_pure_source(self) -> bool {
        matches!(
            self,
            NodeKind::GetVariable | NodeKind::GetCameraByName | NodeKind::GetRecipient
        )
    }

    pub fn input_ports(self) -> &'static [Port] {
        match self {
            NodeKind::GetLastAlert => &NO_PORTS,
            NodeKind::GetRecipient => &NO_PORTS,
            NodeKind::CreateAlert => &CREATE_ALERT_IN,
            NodeKind::DeliverAlert => &DELIVER_ALERT_IN,
            NodeKind::CameraSnapshot => &CAMERA_SNAPSHOT_IN,
            NodeKind::GetTrackedObjects => &GET_TRACKED_OBJECTS_IN,
            NodeKind::GetCurrentFrame => &GET_CURRENT_FRAME_IN,
            NodeKind::GetCameraByName => &NO_PORTS,
            NodeKind::BreakCamera => &BREAK_CAMERA_IN,
            NodeKind::Region => &REGION_IN,
            NodeKind::DescribeImage => &DESCRIBE_IMAGE_IN,
            NodeKind::NotifyRecipient => &NOTIFY_RECIPIENT_IN,
            NodeKind::Format => &FORMAT_IN,
            NodeKind::Wait => &NO_PORTS, // control-flow only — sequenced via exec ports
            NodeKind::HttpPost => &HTTP_POST_IN,
            NodeKind::If => &IF_IN,
            NodeKind::ForEach => &FOR_EACH_IN,
            NodeKind::MotionDetection => &MOTION_IN,
            NodeKind::MotionWatch => &MOTION_WATCH_IN,
            NodeKind::PersonDetection => &PERSON_DETECTION_IN,
            NodeKind::ObjectDetection => &OBJECT_DETECTION_IN,
            NodeKind::BreakPersonDetection => &BREAK_PERSON_IN,
            NodeKind::BreakTrackedObject => &BREAK_TRACKED_OBJECT_IN,
            NodeKind::BreakDetection => &BREAK_DETECTION_IN,
            NodeKind::BreakFaceDetection => &BREAK_FACE_IN,
            NodeKind::RecognizePerson => &RECOGNIZE_PERSON_IN,
            NodeKind::FaceEmbedding => &FACE_EMBEDDING_IN,
            NodeKind::RecognizeFace => &RECOGNIZE_FACE_IN,
            NodeKind::StoreFace => &STORE_FACE_IN,
            NodeKind::RecordPresence => &RECORD_PRESENCE_IN,
            NodeKind::EmitEvent => &NO_PORTS, // exec-only (universal exec_in/out); event name in config
            NodeKind::OnEvent => &NO_PORTS,   // entry: only an exec_out; activated by the event
            NodeKind::Print => &PRINT_IN,
            NodeKind::ShowImage => &SHOW_IMAGE_IN,
            NodeKind::DrawBbox => &DRAW_BBOX_IN,
            NodeKind::DrawText => &DRAW_TEXT_IN,
            NodeKind::BlurOutsideBbox => &BLUR_OUTSIDE_BBOX_IN,
            NodeKind::GetImageSize => &GET_IMAGE_SIZE_IN,
            NodeKind::And | NodeKind::Or => &BOOL_OP_IN,
            NodeKind::Not => &NOT_IN,
            NodeKind::TextCompare => &TEXT_COMPARE_IN,
            NodeKind::FlipFlop => &NO_PORTS, // engine-driven; no data inputs
            NodeKind::Compare
            | NodeKind::NumLt
            | NodeKind::NumLte
            | NodeKind::NumGt
            | NodeKind::NumGte
            | NodeKind::NumEq
            | NodeKind::NumNe => &COMPARE_IN,
            NodeKind::NumToText => &NUM_TO_TEXT_IN,
            NodeKind::BoolToText => &BOOL_TO_TEXT_IN,
            NodeKind::Cooldown => &NO_PORTS, // engine-driven; window_secs is config
            NodeKind::StringSwitch => &STRING_SWITCH_IN,
            NodeKind::CameraSwitch => &CAMERA_SWITCH_IN,
            NodeKind::AskAi => &ASK_AI_IN,
            NodeKind::Comment => &NO_PORTS,
            NodeKind::Reroute => &REROUTE_IN,
            NodeKind::StartRecording => &START_RECORDING_IN,
            NodeKind::StopRecording => &STOP_RECORDING_IN,
            NodeKind::TimeGate => &NO_PORTS, // exec-only; wait_secs/extend_secs are config
            NodeKind::DelayTrigger => &NO_PORTS, // exec-only; wait_secs/extend_secs are config
            // Get has no inputs; Set's typed `value` input is dynamic (GraphNode::input_ports).
            NodeKind::GetVariable | NodeKind::SetVariable => &NO_PORTS,
            NodeKind::ReplyToSender => &REPLY_TO_SENDER_IN,
            NodeKind::AiAnswer => &AI_ANSWER_IN,
            NodeKind::ReplyWithRecording => &REPLY_RECORDING_IN,
            NodeKind::AiSwitch => &AI_SWITCH_IN,
            NodeKind::AiExtract => &AI_EXTRACT_IN,
            NodeKind::DictRowCount => &DICT_ROW_COUNT_IN,
            NodeKind::DictGetRow => &DICT_GET_ROW_IN,
            NodeKind::DictFindRow => &DICT_FIND_ROW_IN,
            NodeKind::DictGetCell => &DICT_GET_CELL_IN,
            NodeKind::DictAddRow => &DICT_ADD_ROW_IN,
            NodeKind::DictSetCell => &DICT_SET_CELL_IN,
            NodeKind::DictUpdateRow => &DICT_UPDATE_ROW_IN,
            NodeKind::DictDeleteRow => &DICT_DELETE_ROW_IN,
            NodeKind::DictClear => &DICT_CLEAR_IN,
            NodeKind::DictSchema => &NO_PORTS, // inputs come from config columns only
            // DictMakeRow's inputs are dynamic (schema + one per column, see GraphNode::input_ports);
            // this static list is the fallback = just `schema`.
            NodeKind::DictMakeRow => &DICT_MAKE_ROW_IN,
            NodeKind::DictAppendRow => &DICT_APPEND_ROW_IN,
            // Device + data point + value all come from config dropdowns (no data inputs).
            NodeKind::TuyaControl | NodeKind::TuyaReadStatus => &NO_PORTS,
        }
    }

    pub fn output_ports(self) -> &'static [Port] {
        match self {
            NodeKind::GetLastAlert => &GET_LAST_ALERT_OUT,
            NodeKind::GetRecipient => &GET_RECIPIENT_OUT,
            NodeKind::CreateAlert => &CREATE_ALERT_OUT,
            NodeKind::DeliverAlert => &DELIVER_ALERT_OUT,
            NodeKind::CameraSnapshot => &CAMERA_SNAPSHOT_OUT,
            NodeKind::GetTrackedObjects => &GET_TRACKED_OBJECTS_OUT,
            NodeKind::GetCurrentFrame => &GET_CURRENT_FRAME_OUT,
            NodeKind::GetCameraByName => &GET_CAMERA_BY_NAME_OUT,
            NodeKind::BreakCamera => &BREAK_CAMERA_OUT,
            NodeKind::Region => &REGION_OUT,
            NodeKind::DescribeImage => &DESCRIBE_IMAGE_OUT,
            NodeKind::NotifyRecipient => &NOTIFY_RECIPIENT_OUT,
            NodeKind::Format => &FORMAT_OUT,
            NodeKind::Wait => &NO_PORTS, // control-flow only — sequenced via exec ports
            NodeKind::HttpPost => &HTTP_POST_OUT,
            NodeKind::If => &NO_PORTS, // outputs are exec branches (true/false)
            NodeKind::ForEach => &FOR_EACH_OUT, // item + index (exec outs are loop_body/completed)
            NodeKind::MotionDetection => &MOTION_OUT,
            NodeKind::MotionWatch => &MOTION_OUT,
            NodeKind::PersonDetection => &PERSON_DETECTION_OUT,
            NodeKind::ObjectDetection => &OBJECT_DETECTION_OUT,
            NodeKind::BreakPersonDetection => &BREAK_PERSON_OUT,
            NodeKind::BreakTrackedObject => &BREAK_TRACKED_OBJECT_OUT,
            NodeKind::BreakDetection => &BREAK_DETECTION_OUT,
            NodeKind::BreakFaceDetection => &BREAK_FACE_OUT,
            NodeKind::RecognizePerson => &RECOGNIZE_PERSON_OUT,
            NodeKind::FaceEmbedding => &FACE_EMBEDDING_OUT,
            NodeKind::RecognizeFace => &RECOGNIZE_FACE_OUT,
            NodeKind::StoreFace => &STORE_FACE_OUT,
            NodeKind::RecordPresence => &RECORD_PRESENCE_OUT,
            NodeKind::EmitEvent => &NO_PORTS, // exec passes through, no data out
            NodeKind::OnEvent => &NO_PORTS,   // drives its subtree via exec_out only
            NodeKind::Print => &NO_PORTS,     // a sink — exec passes through, no data out
            NodeKind::ShowImage => &NO_PORTS, // a sink — exec passes through, no data out
            NodeKind::DrawBbox => &DRAW_BBOX_OUT,
            NodeKind::DrawText => &DRAW_TEXT_OUT,
            NodeKind::BlurOutsideBbox => &BLUR_OUTSIDE_BBOX_OUT,
            NodeKind::GetImageSize => &GET_IMAGE_SIZE_OUT,
            NodeKind::And | NodeKind::Or | NodeKind::Not | NodeKind::TextCompare => &COMPARE_OUT,
            NodeKind::FlipFlop => &FLIP_FLOP_OUT,
            NodeKind::Compare
            | NodeKind::NumLt
            | NodeKind::NumLte
            | NodeKind::NumGt
            | NodeKind::NumGte
            | NodeKind::NumEq
            | NodeKind::NumNe => &COMPARE_OUT,
            NodeKind::NumToText | NodeKind::BoolToText => &CAST_TEXT_OUT,
            NodeKind::Cooldown => &COOLDOWN_OUT,
            NodeKind::StringSwitch | NodeKind::CameraSwitch => &NO_PORTS, // exec outputs are dynamic arms (+ "default")
            NodeKind::AskAi => &NO_PORTS, // exec outputs are true/false branches only
            NodeKind::Comment => &NO_PORTS,
            NodeKind::Reroute => &REROUTE_OUT,
            NodeKind::StartRecording => &START_RECORDING_OUT,
            NodeKind::StopRecording => &NO_PORTS,
            NodeKind::TimeGate => &NO_PORTS, // exec-only; fires via exec "out" port
            NodeKind::DelayTrigger => &NO_PORTS, // exec-only; fires via exec "start"/"stop"
            // Get's typed `value` output is dynamic (GraphNode::output_ports); Set has none.
            NodeKind::GetVariable | NodeKind::SetVariable => &NO_PORTS,
            NodeKind::ReplyToSender => &REPLY_TO_SENDER_OUT,
            NodeKind::AiAnswer => &AI_ANSWER_OUT,
            NodeKind::ReplyWithRecording => &REPLY_RECORDING_OUT,
            NodeKind::AiSwitch => &NO_PORTS, // exec outputs are dynamic arms (+ "default")
            NodeKind::AiExtract => &AI_EXTRACT_OUT, // `value` type is refined per config (GraphNode)
            NodeKind::DictRowCount => &DICT_ROW_COUNT_OUT,
            NodeKind::DictGetRow => &DICT_GET_ROW_OUT,
            NodeKind::DictFindRow => &DICT_FIND_ROW_OUT,
            NodeKind::DictGetCell => &DICT_GET_CELL_OUT,
            // Mutate nodes: exec passes through, no data out.
            NodeKind::DictAddRow
            | NodeKind::DictSetCell
            | NodeKind::DictUpdateRow
            | NodeKind::DictDeleteRow
            | NodeKind::DictClear => &NO_PORTS,
            // Build nodes: pure, emit a composed value.
            NodeKind::DictSchema => &DICT_SCHEMA_OUT,
            NodeKind::DictMakeRow => &DICT_MAKE_ROW_OUT,
            NodeKind::DictAppendRow => &DICT_APPEND_ROW_OUT,
            NodeKind::TuyaControl => &TUYA_CONTROL_OUT,
            NodeKind::TuyaReadStatus => &TUYA_READ_STATUS_OUT,
        }
    }

    pub fn input_port(self, name: &str) -> Option<Port> {
        if name == EXEC_IN.name {
            return Some(EXEC_IN);
        }
        self.input_ports().iter().copied().find(|p| p.name == name)
    }

    pub fn output_port(self, name: &str) -> Option<Port> {
        if let Some(e) = self
            .exec_out_ports()
            .iter()
            .copied()
            .find(|p| p.name == name)
        {
            return Some(e);
        }
        self.output_ports().iter().copied().find(|p| p.name == name)
    }

    /// Static config + literal fallbacks for unconnected input ports.
    pub fn default_config(self) -> Value {
        match self {
            NodeKind::GetLastAlert => json!({}),
            NodeKind::GetRecipient => json!({ "recipient": "" }),
            NodeKind::CreateAlert => {
                json!({ "message": "", "room": "", "level": "info", "source": "workflow", "timeout_secs": "15" })
            }
            NodeKind::DeliverAlert => json!({ "id": "", "timeout_secs": "35" }),
            NodeKind::CameraSnapshot => json!({ "camera": "first", "timeout_secs": "10" }),
            NodeKind::GetTrackedObjects => json!({ "camera": "first", "timeout_secs": "10" }),
            NodeKind::GetCurrentFrame => {
                json!({ "camera": "first", "max_age_ms": "2000", "timeout_secs": "10" })
            }
            NodeKind::GetCameraByName => json!({ "camera": "" }),
            NodeKind::BreakCamera => json!({}),
            NodeKind::Region => json!({ "camera": "first", "name": "", "points": [] }),
            NodeKind::DescribeImage => json!({ "label": "", "question": "", "timeout_secs": "65" }),
            NodeKind::NotifyRecipient => json!({ "message": "", "timeout_secs": "35" }),
            NodeKind::Format => json!({ "template": "{p1} — {p2}", "p1": "", "p2": "", "p3": "" }),
            NodeKind::Wait => json!({ "seconds": "1" }),
            NodeKind::HttpPost => {
                json!({ "url": "", "headers": {}, "fields": {}, "file": "", "timeout_secs": "35" })
            }
            NodeKind::If => json!({}),
            NodeKind::ForEach => json!({ "items": "" }),
            NodeKind::MotionDetection => {
                json!({ "sensitivity": "2", "pixel_threshold": "25", "timeout_secs": "30" })
            }
            NodeKind::MotionWatch => {
                json!({ "camera": "first", "sensitivity": "2", "pixel_threshold": "25", "sample_gap_ms": "400", "timeout_secs": "15" })
            }
            NodeKind::PersonDetection => json!({ "timeout_secs": "30" }),
            NodeKind::ObjectDetection => json!({ "class": "person", "timeout_secs": "30" }),
            NodeKind::BreakPersonDetection => json!({}),
            NodeKind::BreakTrackedObject => json!({}),
            NodeKind::BreakDetection => json!({}),
            NodeKind::BreakFaceDetection => json!({}),
            NodeKind::RecognizePerson => json!({ "max_distance": "0.4", "timeout_secs": "30" }),
            NodeKind::FaceEmbedding => json!({ "timeout_secs": "30" }),
            NodeKind::RecognizeFace => json!({ "top_k": "5", "max_distance": "0.5", "timeout_secs": "15" }),
            NodeKind::StoreFace => json!({ "source": "snapshot", "dedup_distance": "0.15", "timeout_secs": "15" }),
            NodeKind::RecordPresence => {
                json!({ "category": "person", "room": "", "label": "", "duration_secs": "8" })
            }
            NodeKind::EmitEvent => json!({ "event": "" }),
            NodeKind::OnEvent => json!({ "event": "" }),
            NodeKind::Print => json!({ "message": "", "on_screen": true, "level": "info" }),
            NodeKind::ShowImage => json!({ "label": "" }),
            NodeKind::FlipFlop => json!({}),
            NodeKind::Compare => json!({ "operator": "==", "a": "0", "b": "0" }),
            NodeKind::NumLt
            | NodeKind::NumLte
            | NodeKind::NumGt
            | NodeKind::NumGte
            | NodeKind::NumEq
            | NodeKind::NumNe => json!({ "a": "0", "b": "0" }),
            NodeKind::NumToText | NodeKind::BoolToText => json!({}),
            NodeKind::Cooldown => json!({ "window_secs": "60" }),
            NodeKind::DrawBbox => {
                json!({ "color": "#00FF00", "thickness": "2", "style": "box", "expand": "0" })
            }
            NodeKind::DrawText => {
                json!({ "text": "", "position": "top-left", "size": "18", "text_color": "#FFFFFF", "background": true, "bg_color": "#000000" })
            }
            NodeKind::BlurOutsideBbox => json!({ "blur": "12" }),
            NodeKind::GetImageSize => json!({}),
            NodeKind::And | NodeKind::Or => json!({ "a": false, "b": false }),
            NodeKind::Not => json!({ "a": false }),
            NodeKind::TextCompare => json!({ "operator": "eq", "b": "" }),
            NodeKind::StringSwitch => json!({ "arms": ["arm_1", "arm_2"] }),
            NodeKind::CameraSwitch => json!({ "arms": [] }),
            NodeKind::AskAi => json!({ "question": "", "timeout_secs": "65" }),
            NodeKind::Comment => {
                json!({ "text": "Comment", "color": "#facc15", "width": 400, "height": 200 })
            }
            NodeKind::Reroute => json!({}),
            NodeKind::StartRecording => json!({ "timeout_secs": "30" }),
            NodeKind::StopRecording => json!({ "timeout_secs": "15" }),
            NodeKind::TimeGate => json!({ "wait_secs": 10, "extend_secs": 10 }),
            NodeKind::DelayTrigger => json!({ "wait_secs": 10, "extend_secs": 10 }),
            // `variable` = the declared variable's name; `type` = its wire type (baked in so the
            // dynamic port resolves without a graph lookup). Set by the Variables panel on create.
            NodeKind::GetVariable | NodeKind::SetVariable => {
                json!({ "variable": "", "type": "text" })
            }
            NodeKind::ReplyToSender => json!({ "message": "", "timeout_secs": "35" }),
            NodeKind::AiAnswer => json!({ "prompt": "", "timeout_secs": "30" }),
            NodeKind::ReplyWithRecording => json!({ "caption": "", "timeout_secs": "120" }),
            NodeKind::AiSwitch => json!({ "arms": ["arm_1", "arm_2"], "timeout_secs": "30" }),
            NodeKind::AiExtract => json!({ "field": "", "type": "text", "timeout_secs": "30" }),
            // Dictionaries: `table` is a dictionary id (dropdown); the rest are literal fallbacks
            // for the unwired input ports.
            NodeKind::DictRowCount | NodeKind::DictClear => json!({ "table": "" }),
            NodeKind::DictGetRow => json!({ "table": "", "index": "0" }),
            NodeKind::DictFindRow => json!({ "table": "", "column": "", "key": "" }),
            NodeKind::DictGetCell => json!({ "table": "", "row": "0", "column": "" }),
            NodeKind::DictAddRow => json!({ "table": "", "values": {} }),
            NodeKind::DictSetCell => json!({ "table": "", "row": "0", "column": "", "value": "" }),
            NodeKind::DictUpdateRow => json!({ "table": "", "column": "", "key": "", "values": {} }),
            NodeKind::DictDeleteRow => json!({ "table": "", "column": "", "key": "" }),
            // Build nodes compose a table in-graph. `columns` = `[{name, type}]` (same shape as a
            // Dictionary's schema); DictSchema/DictMakeRow bake it, DictAppendRow is input-only.
            NodeKind::DictSchema | NodeKind::DictMakeRow => json!({ "columns": [] }),
            NodeKind::DictAppendRow => json!({}),
            // `tuya_device` = the device dropdown (stores the stable device id); `tuya_code` =
            // the data point dropdown (e.g. `switch_1`); `value` = the value literal, rendered
            // by an adaptive widget typed to the point.
            NodeKind::TuyaControl => json!({ "tuya_device": "", "tuya_code": "", "value": "", "value_type": "bool", "timeout_secs": "15" }),
            NodeKind::TuyaReadStatus => json!({ "tuya_device": "", "tuya_code": "", "value_type": "text", "timeout_secs": "15" }),
        }
    }
}

// ---------------------------------------------------------------------------
// Graph elements
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GraphNode {
    pub id: u32,
    pub kind: NodeKind,
    /// Canvas position (the Svelte editor owns the units; persisted so layout survives).
    pub x: i32,
    pub y: i32,
    /// Static settings + literal fallbacks for unconnected input ports.
    #[serde(default)]
    pub config: Value,
    /// Whether this node's output is cached for the duration of a run (default) or
    /// recomputed every time the node is reached. `true` = today's behavior (runs once,
    /// reused on fan-out). Set `false` for loop bodies and time-varying sources
    /// (e.g. `GetCurrentFrame`) so re-activation actually re-runs them. Old graphs
    /// (no field) deserialize to `true`.
    #[serde(default = "default_true")]
    pub memoize: bool,
}

fn default_true() -> bool {
    true
}

impl GraphNode {
    /// This node's **effective** input ports. Static for every kind except `EmitEvent`,
    /// whose inputs are one-per-payload-field derived from its config (see [`payload_ports`]).
    pub fn input_ports(&self) -> Vec<ResolvedPort> {
        match self.kind {
            NodeKind::EmitEvent => payload_ports(&self.config),
            // Set Variable: one `value` input typed by the variable (required).
            NodeKind::SetVariable => variable_port(&self.config, true),
            // Control Tuya Device: one `value` input typed by the selected data point (optional —
            // config literal is the fallback). See `tuya_value_port`.
            NodeKind::TuyaControl => tuya_value_port(&self.config),
            // OnEvent: camera/region inputs, only for the edge tracker's events (see the fn doc).
            NodeKind::OnEvent => on_event_input_ports(&self.config),
            // DictMakeRow: `schema` (Table) + one input per configured column (see `dict_row_ports`).
            NodeKind::DictMakeRow => dict_row_ports(&self.config),
            k => k.input_ports().iter().map(ResolvedPort::from).collect(),
        }
    }

    /// This node's **effective** output ports. Static for every kind except `OnEvent`,
    /// whose outputs are one-per-payload-field derived from its config.
    pub fn output_ports(&self) -> Vec<ResolvedPort> {
        match self.kind {
            NodeKind::OnEvent => payload_ports(&self.config),
            // Get Variable: one `value` output typed by the variable.
            NodeKind::GetVariable => variable_port(&self.config, false),
            // Read Tuya Status: `value` typed by the selected data point (+ static `on`/`raw`).
            NodeKind::TuyaReadStatus => tuya_read_output_ports(&self.config),
            // AI Extract: `value` typed per config.type (like a variable) + a static `found` bool.
            NodeKind::AiExtract => {
                let mut ports = variable_port(&self.config, false);
                ports.push(ResolvedPort { name: "found".to_string(), ty: PortType::Bool, required: false });
                ports
            }
            k => k.output_ports().iter().map(ResolvedPort::from).collect(),
        }
    }

    /// Look up one input port by name (config-aware), incl. the universal `exec_in`.
    pub fn input_port(&self, name: &str) -> Option<ResolvedPort> {
        if name == EXEC_IN.name {
            return Some(ResolvedPort::from(&EXEC_IN));
        }
        self.input_ports().into_iter().find(|p| p.name == name)
    }

    /// Look up one output port by name (config-aware), incl. the kind's exec_out pins.
    pub fn output_port(&self, name: &str) -> Option<ResolvedPort> {
        // StringSwitch / AiSwitch: exec outputs are user-defined arm labels + the fixed "default"
        // arm. These are instance-level (stored in config), not on the static kind. AiSwitch shares
        // StringSwitch's arm shape — it just picks the arm by meaning instead of exact match.
        if self.kind == NodeKind::StringSwitch || self.kind == NodeKind::AiSwitch {
            let arms = string_switch_arms(&self.config);
            if arms.iter().any(|a| a == name) || name == "default" {
                return Some(ResolvedPort {
                    name: name.to_string(),
                    ty: PortType::Exec,
                    required: false,
                });
            }
            return None;
        }
        // CameraSwitch: exec outputs are numbered "arm_1", "arm_2", ... + "default".
        if self.kind == NodeKind::CameraSwitch {
            let arms = camera_switch_arms(&self.config);
            let valid =
                (1..=arms.len()).any(|i| name == format!("arm_{i}").as_str()) || name == "default";
            if valid {
                return Some(ResolvedPort {
                    name: name.to_string(),
                    ty: PortType::Exec,
                    required: false,
                });
            }
            return None;
        }
        if let Some(e) = self.kind.exec_out_ports().iter().find(|p| p.name == name) {
            return Some(ResolvedPort::from(e));
        }
        self.output_ports().into_iter().find(|p| p.name == name)
    }
}

/// Extract the arm labels from a `StringSwitch` node's config. The "default" arm is
/// implicit — it fires when no arm label matches — and is NOT included in this list.
pub fn string_switch_arms(config: &Value) -> Vec<String> {
    config
        .get("arms")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.trim().to_string()))
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// Extract the camera keys from a `CameraSwitch` node's config (one per arm; may be empty
/// strings for unconfigured arms). Arm port names are "arm_1", "arm_2", etc. (NOT the key).
/// The "default" arm is implicit — fires when no arm matches — and is NOT included here.
pub fn camera_switch_arms(config: &Value) -> Vec<String> {
    config
        .get("arms")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default()
}

/// A typed, directed wire: source node's `from_port` → target node's `to_port`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Edge {
    pub from: u32,
    pub from_port: String,
    pub to: u32,
    pub to_port: String,
}

/// How a graph fires. A property of the engine/graph — NOT a node.
///
/// Alert reactivity used to live here as `OnNewAlert`; it was retired in favor of the
/// cross-graph event system (a future `System-Alerts` event an `OnEvent` can listen for).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Trigger {
    /// Only fired by Run-now.
    Manual,
    /// Engine poller starts a fresh one-shot run every `every_secs`.
    Cron { every_secs: u64 },
    /// Calendar schedule (#111) — fire at wall-clock times in a timezone (daily at 9am, weekly on
    /// Mon/Wed, …). Durable via the `work_jobs` queue; next-occurrence math in [`super::schedule`].
    Schedule(Schedule),
}

/// A recurring calendar schedule for a graph. Wall-clock, timezone-aware. Minute granularity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Schedule {
    pub freq: ScheduleFreq,
    /// IANA timezone (e.g. `America/Sao_Paulo`); empty → UTC.
    #[serde(default)]
    pub tz: String,
    /// Local times of day to fire at, each `"HH:MM"`. Used by Daily/Weekly/Monthly/Yearly (one entry
    /// = "daily at a time"; several = "N times a day"). Ignored by Hourly.
    #[serde(default)]
    pub times: Vec<String>,
    /// Hourly: the minute of each hour to fire (0–59).
    #[serde(default)]
    pub minute: u8,
    /// Weekly: weekdays to fire on, 0=Mon … 6=Sun.
    #[serde(default)]
    pub weekdays: Vec<u8>,
    /// Monthly/Yearly: day of month (1–31; clamped to the month's length).
    #[serde(default)]
    pub day_of_month: u8,
    /// Yearly: month (1–12).
    #[serde(default)]
    pub month: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScheduleFreq {
    Hourly,
    Daily,
    Weekly,
    Monthly,
    Yearly,
}

impl Default for Trigger {
    fn default() -> Self {
        Trigger::Manual
    }
}

/// What kicked off a run — recorded on every run so the UI can show who/what triggered it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TriggerSource {
    Manual,
    Cron,
    /// A cross-graph (or future system) event fired this graph's `OnEvent` handler.
    Event,
    /// A durable timer came due and forced a run at one node — today, a `DelayTrigger`
    /// whose quiet window elapsed, delivering its trailing `stop`.
    Timer,
}

impl TriggerSource {
    pub fn as_str(self) -> &'static str {
        match self {
            TriggerSource::Manual => "manual",
            TriggerSource::Cron => "cron",
            TriggerSource::Event => "event",
            TriggerSource::Timer => "timer",
        }
    }
}

/// Terminal state of a run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RunStatus {
    Running,
    Ok,
    Failed,
}

impl RunStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            RunStatus::Running => "running",
            RunStatus::Ok => "ok",
            RunStatus::Failed => "failed",
        }
    }
}

/// Who emitted an event into the tenant bus. A graph today; system code (login,
/// user-create, chat, alerts…) in the future via the same fan-out.
#[derive(Debug, Clone, PartialEq)]
pub enum EventSource {
    Graph { id: Uuid, name: String },
    System { label: String },
}

/// What a graph runs **one of** — its instance identity, in Unreal's sense: the graph is the
/// Blueprint, an instance is the actor. Latent state (a `DelayTrigger`'s window) and the
/// concurrency gate are both scoped to an instance, so two instances never collide.
///
/// Before this existed, everything was keyed `(tenant, graph, …)` with no context, which meant one
/// window and one in-flight run per graph *forever*. Two cameras through one recording graph:
/// B's event was coalesce-dropped while A ran, and if it did get through, B couldn't arm its own
/// window — it silently extended A's, and B's clip was never stopped. Single-camera graphs worked
/// by accident.
///
/// A latent node never asks "which camera"; it asks for **its instance's** window. That's what
/// keeps `DelayTrigger` a Control node that knows nothing about cameras.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum InstanceScope {
    /// One instance per graph — byte-for-byte the behaviour before instances existed. Default, so
    /// every existing graph is unaffected.
    Single,
    /// One instance per camera: the event's camera is the instance key. Same camera re-triggers the
    /// same window (the debounce survives); different cameras get their own window and their own
    /// clip.
    PerCamera,
}

impl Default for InstanceScope {
    fn default() -> Self {
        InstanceScope::Single
    }
}

impl InstanceScope {
    /// This signal's instance key. `""` is the single instance — never a missing value: a
    /// `PerCamera` graph fired by an event with no camera has one honest instance, not a broken one.
    pub fn key_for(&self, payload: &crate::workflows::PortValues) -> String {
        match self {
            InstanceScope::Single => String::new(),
            InstanceScope::PerCamera => payload
                .get("camera")
                .map(|v| match v {
                    // The wire carries a camera as Text (ip/id) before `seed_event_payload` upgrades
                    // it; after, it's a CameraEntity. Accept both — the arm can happen either side.
                    crate::workflows::PortValue::Camera(c) => c.id.clone(),
                    other => other.as_text().unwrap_or_default().to_string(),
                })
                .unwrap_or_default(),
        }
    }
}

/// What happens when a graph fires while a previous run of the same graph is in flight.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ConcurrencyPolicy {
    /// ≤1 run; a fire while running is dropped.
    SkipIfRunning,
    /// ≤1 run; extra fires queue up to `max_queued`, then drop.
    QueueSingle { max_queued: u32 },
    /// Up to `max_concurrent` runs at once; a fire at capacity is dropped.
    Concurrent { max_concurrent: u32 },
}

impl Default for ConcurrencyPolicy {
    fn default() -> Self {
        ConcurrencyPolicy::SkipIfRunning
    }
}

// ---------------------------------------------------------------------------
// Graph
// ---------------------------------------------------------------------------

/// The editor's saved camera position — pan offset (px) + zoom — persisted per
/// graph so re-opening it restores where the operator left off.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Viewport {
    pub x: f64,
    pub y: f64,
    pub zoom: f64,
}

impl Default for Viewport {
    fn default() -> Self {
        Viewport {
            x: 40.0,
            y: 40.0,
            zoom: 1.0,
        }
    }
}

/// One named automation: a trigger + a DAG of typed nodes/edges.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Graph {
    pub id: Uuid,
    pub name: String,
    /// One-line summary shown as a caption under the name in the list. Additive —
    /// old graphs deserialize with "".
    #[serde(default)]
    pub description: String,
    /// Organisational label (a "group"). Workflows sharing a group cluster together in
    /// the list, like log modules. Empty = ungrouped. Additive.
    #[serde(default)]
    pub group: String,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub trigger: Trigger,
    #[serde(default)]
    pub concurrency: ConcurrencyPolicy,
    /// What this graph runs one of (see [`InstanceScope`]). Additive: old graphs deserialize to
    /// `Single`, which is exactly how they behaved before instances existed.
    #[serde(default)]
    pub instance_scope: InstanceScope,
    /// Run nodes that have no edges (orphans on the canvas). Default `false` —
    /// orphans are skipped so leftover/experimental nodes don't fire.
    #[serde(default)]
    pub run_orphans: bool,
    /// Emit a per-node debug log line during a run (for the editor console).
    #[serde(default)]
    pub debug: bool,
    /// This graph is a **template** — a starting point to clone, not a graph that runs on its own
    /// (card #102). Shown in the templates picker; "Use template" duplicates it into a normal
    /// workflow. A user's own template is tenant-scoped like any graph. Additive.
    #[serde(default)]
    pub template: bool,
    /// A **global** template — visible to *every* tenant, not just its owner. Only a master can set
    /// this (the auth check is in control); it lives in the master's own tenant but is read
    /// cross-tenant for the catalog. Meaningless unless `template` is also true. Additive.
    #[serde(default)]
    pub global: bool,
    #[serde(default = "one")]
    pub next_id: u32,
    #[serde(default)]
    pub nodes: Vec<GraphNode>,
    #[serde(default)]
    pub edges: Vec<Edge>,
    #[serde(default)]
    pub viewport: Viewport,
    /// Visual wire bend/reroute handles per edge (card #1), keyed `"from:from_port:to:to_port"`,
    /// each a list of `[x, y]` world points. Purely cosmetic — the logical edge is unchanged — so
    /// the user's wire organisation survives a reload. Additive (`{}` on old graphs).
    #[serde(default)]
    pub edge_vias: std::collections::HashMap<String, Vec<[i32; 2]>>,
    /// Typed, per-graph variables (Unreal-style). Each spawns Get/Set Variable nodes whose
    /// port type follows its declared type. Additive — old graphs deserialize with `[]`.
    #[serde(default)]
    pub variables: Vec<VariableDecl>,
}

/// A workflow variable declaration: a name + its wire type. Declared in the editor's Variables
/// panel; referenced by `GetVariable`/`SetVariable` nodes (which bake the name+type into config).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VariableDecl {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: PortType,
    /// Optional author-defined default value (as a string, interpreted per `ty`). Seeds the run's
    /// variables map at run start so a `Get` before any `Set` returns it. Additive (`None` on old
    /// graphs). Card #5b.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
    /// This variable is a user-facing **input** — the UE "expose as input" flag (card #102). Pure
    /// metadata: the engine doesn't branch on it (an input's value IS its `default`, optionally
    /// overridden per manual run). Drives the setup form + the run-time prompt. Additive.
    #[serde(default)]
    pub param: bool,
    /// Help text for the input, shown beside it in the setup form. Additive.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

fn one() -> u32 {
    1
}

impl Graph {
    pub fn new(name: &str) -> Self {
        Graph {
            template: false,
            global: false,
            instance_scope: InstanceScope::default(),
            id: Uuid::new_v4(),
            name: name.to_string(),
            description: String::new(),
            group: String::new(),
            enabled: false,
            trigger: Trigger::Manual,
            concurrency: ConcurrencyPolicy::SkipIfRunning,
            run_orphans: false,
            debug: false,
            next_id: 1,
            nodes: Vec::new(),
            edges: Vec::new(),
            viewport: Viewport::default(),
            edge_vias: std::collections::HashMap::new(),
            variables: Vec::new(),
        }
    }

    pub fn node(&self, id: u32) -> Option<&GraphNode> {
        self.nodes.iter().find(|n| n.id == id)
    }

    pub fn node_mut(&mut self, id: u32) -> Option<&mut GraphNode> {
        self.nodes.iter_mut().find(|n| n.id == id)
    }

    /// The node feeding `to_port` on `to`, if wired — the single generic building block behind
    /// every "resolve what a node's input currently refers to" lookup (camera, region, …).
    /// Graph-static: reads only config, no execution — every producer this is used to resolve
    /// (`GetCameraByName`, `Region`) is a pure, config-only node, so its value is 100%
    /// determined by the graph alone, without running anything.
    pub fn wired_source(&self, to: u32, to_port: &str) -> Option<&GraphNode> {
        let edge = self.edges.iter().find(|e| e.to == to && e.to_port == to_port)?;
        self.node(edge.from)
    }

    /// What `node`'s `camera` input currently resolves to (an id/ip/name selector, NOT yet
    /// resolved to a device — callers do that against their own device list): the wired
    /// `GetCameraByName` node's own `config.camera`, or (no wire) `node`'s OWN `config.camera`
    /// literal — the same wire-overrides-config pattern as `ObjectDetection`'s `class` input.
    /// `None` if neither is set, meaning "unscoped" to callers like `on_event_node`.
    pub fn resolve_camera_selector(&self, node: &GraphNode) -> Option<String> {
        let cfg_str = |n: &GraphNode| n.config.get("camera").and_then(|v| v.as_str()).map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
        self.wired_source(node.id, "camera")
            .filter(|n| n.kind == NodeKind::GetCameraByName)
            .and_then(cfg_str)
            .or_else(|| cfg_str(node))
    }

    /// What an `OnEvent`'s `region` input currently resolves to: the wired `Region` node's own
    /// `name` + `points`. `None` if nothing's wired — unlike camera, a region has no
    /// literal-picker fallback (there's no sensible way to hand-type a polygon), so it's wired or
    /// it doesn't exist for this node. The `Region` node's OWN `camera` field is deliberately NOT
    /// consulted here — it's only a preview reference for that node's drawing canvas; the job's
    /// camera is always the reading `OnEvent`'s own (`resolve_camera_selector`), so a region
    /// wired into a node scoped to a different camera is on the caller to catch, not silently
    /// cross-applied here.
    pub fn resolve_region(&self, node: &GraphNode) -> Option<(String, Vec<(f32, f32)>)> {
        let region_node = self.wired_source(node.id, "region").filter(|n| n.kind == NodeKind::Region)?;
        let name = region_node
            .config
            .get("name")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())?
            .to_string();
        let points: Vec<(f32, f32)> = region_node
            .config
            .get("points")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|p| {
                        let a = p.as_array()?;
                        Some((a.first()?.as_f64()? as f32, a.get(1)?.as_f64()? as f32))
                    })
                    .collect()
            })
            .unwrap_or_default();
        if points.len() < 2 {
            return None;
        }
        Some((name, points))
    }

    /// Add a node at (x, y); returns its new id. All new nodes default to memoize=false
    /// (recompute every time) — the user opts in to caching explicitly via the inspector.
    pub fn add_node(&mut self, kind: NodeKind, x: i32, y: i32) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        self.nodes.push(GraphNode {
            id,
            kind,
            x,
            y,
            config: kind.default_config(),
            memoize: false,
        });
        id
    }

    /// Remove a node and any edge that touches it.
    pub fn remove_node(&mut self, id: u32) {
        self.nodes.retain(|n| n.id != id);
        self.edges.retain(|e| e.from != id && e.to != id);
    }

    /// Distinct downstream node ids reachable in one hop, in edge order.
    pub fn children(&self, id: u32) -> Vec<u32> {
        let mut out = Vec::new();
        for e in &self.edges {
            if e.from == id && !out.contains(&e.to) {
                out.push(e.to);
            }
        }
        out
    }

    /// Nodes with no incoming edge (sources / fully-literal nodes).
    pub fn roots(&self) -> Vec<u32> {
        self.nodes
            .iter()
            .map(|n| n.id)
            .filter(|id| !self.edges.iter().any(|e| e.to == *id))
            .collect()
    }

    /// True if `target` is reachable from `start` over node-level edges.
    fn reaches(&self, start: u32, target: u32) -> bool {
        let mut stack = vec![start];
        let mut seen = HashSet::new();
        while let Some(n) = stack.pop() {
            if n == target {
                return true;
            }
            if !seen.insert(n) {
                continue;
            }
            for c in self.children(n) {
                stack.push(c);
            }
        }
        false
    }

    /// Whether an output of type `from` may feed an input of type `to`: an exact match, or
    /// either side is the `Any` wildcard (so `ForEach` accepts any array and yields any
    /// element). Keeps every other pair strictly typed.
    fn compatible(from: PortType, to: PortType) -> bool {
        from == to || from == PortType::Any || to == PortType::Any
    }

    /// Validate and add a typed edge. Rejects: self-loop · unknown port ·
    /// type mismatch · an already-fed input port · a cycle · an exact duplicate.
    pub fn add_edge(
        &mut self,
        from: u32,
        from_port: &str,
        to: u32,
        to_port: &str,
    ) -> Result<(), String> {
        if from == to {
            return Err("a node cannot link to itself".into());
        }
        // Resolve port types from the NODES (config-aware), so payload ports on
        // EmitEvent/OnEvent — derived from the configured event's schema — validate too.
        let fnode = self.node(from).ok_or("source node not found")?;
        let tnode = self.node(to).ok_or("target node not found")?;
        let ft = fnode
            .output_port(from_port)
            .ok_or_else(|| format!("{} has no output port '{from_port}'", fnode.kind.label()))?
            .ty;
        let tt = tnode
            .input_port(to_port)
            .ok_or_else(|| format!("{} has no input port '{to_port}'", tnode.kind.label()))?
            .ty;
        if !Self::compatible(ft, tt) {
            return Err(format!("type mismatch: {ft:?} → {tt:?}"));
        }
        // exec_in accepts fan-in (multiple execution paths converging on the same node —
        // e.g. both branches of an If merging downstream). Data ports stay single-fed.
        let is_exec = to_port == EXEC_IN.name;
        if !is_exec
            && self
                .edges
                .iter()
                .any(|e| e.to == to && e.to_port == to_port)
        {
            return Err(format!("input '{to_port}' is already wired"));
        }
        // The same pin pair twice is a no-op wire the fan-in path would otherwise stack up.
        if self
            .edges
            .iter()
            .any(|e| e.from == from && e.from_port == from_port && e.to == to && e.to_port == to_port)
        {
            return Err("those pins are already wired".into());
        }
        if self.reaches(to, from) && !is_exec {
            return Err("that link would create a data cycle".into());
        }
        self.edges.push(Edge {
            from,
            from_port: from_port.to_string(),
            to,
            to_port: to_port.to_string(),
        });
        Ok(())
    }

    pub fn remove_edge(&mut self, idx: usize) {
        if idx < self.edges.len() {
            self.edges.remove(idx);
        }
    }

    /// Topological node order (stable: follows node insertion order on ties).
    /// `Err` if the graph contains a cycle.
    pub fn topo_order(&self) -> Result<Vec<u32>, String> {
        let mut indeg: HashMap<u32, usize> = self.nodes.iter().map(|n| (n.id, 0)).collect();
        let mut adj: HashMap<u32, Vec<u32>> =
            self.nodes.iter().map(|n| (n.id, Vec::new())).collect();
        let mut seen = HashSet::new();
        for e in &self.edges {
            if seen.insert((e.from, e.to)) {
                if let Some(v) = adj.get_mut(&e.from) {
                    v.push(e.to);
                }
                if let Some(d) = indeg.get_mut(&e.to) {
                    *d += 1;
                }
            }
        }
        let mut order = Vec::with_capacity(self.nodes.len());
        let mut done: HashSet<u32> = HashSet::new();
        while order.len() < self.nodes.len() {
            let next = self
                .nodes
                .iter()
                .map(|n| n.id)
                .find(|id| !done.contains(id) && indeg.get(id).copied().unwrap_or(0) == 0);
            let Some(id) = next else {
                return Err("graph has a cycle".into());
            };
            done.insert(id);
            order.push(id);
            if let Some(children) = adj.get(&id) {
                for c in children {
                    if let Some(d) = indeg.get_mut(c) {
                        *d = d.saturating_sub(1);
                    }
                }
            }
        }
        Ok(order)
    }

    /// The exec **back-edges** (loop-closing edges) as `(from,to)` node pairs, found by a
    /// DFS in node order (an edge to a node still on the recursion stack is a back-edge).
    /// Only exec edges can close a cycle — data cycles are rejected by [`Self::add_edge`].
    /// The engine excludes these from its forward ordering and uses them to re-enter loops.
    pub fn back_edges(&self) -> HashSet<(u32, u32)> {
        let mut color: HashMap<u32, u8> = self.nodes.iter().map(|n| (n.id, 0u8)).collect(); // 0 white · 1 gray (on stack) · 2 black
        let mut back: HashSet<(u32, u32)> = HashSet::new();
        for root in self.nodes.iter().map(|n| n.id) {
            if color.get(&root).copied().unwrap_or(2) != 0 {
                continue;
            }
            color.insert(root, 1);
            let mut stack: Vec<(u32, Vec<u32>, usize)> = vec![(root, self.children(root), 0)];
            loop {
                let action = {
                    let Some(frame) = stack.last_mut() else { break };
                    if frame.2 < frame.1.len() {
                        let c = frame.1[frame.2];
                        frame.2 += 1;
                        Some((frame.0, c))
                    } else {
                        None
                    }
                };
                match action {
                    Some((parent, c)) => match color.get(&c).copied().unwrap_or(2) {
                        0 => {
                            color.insert(c, 1);
                            let cc = self.children(c);
                            stack.push((c, cc, 0));
                        }
                        1 => {
                            back.insert((parent, c)); // child still on the stack → back-edge
                        }
                        _ => {}
                    },
                    None => {
                        if let Some((node, _, _)) = stack.pop() {
                            color.insert(node, 2);
                        }
                    }
                }
            }
        }
        back
    }
}

// ---------------------------------------------------------------------------
// Named-graph collection + persistence
// ---------------------------------------------------------------------------

/// The collection of named graphs (managed state on the Tauri side).
#[derive(Default)]
pub struct Workflows {
    pub graphs: Vec<Graph>,
}

impl Workflows {
    pub fn from_graphs(graphs: Vec<Graph>) -> Self {
        Workflows { graphs }
    }

    pub fn create_graph(&mut self, name: &str) -> Uuid {
        let g = Graph::new(name);
        let id = g.id;
        self.graphs.push(g);
        id
    }

    pub fn remove_graph(&mut self, id: Uuid) {
        self.graphs.retain(|g| g.id != id);
    }

    pub fn graph(&self, id: Uuid) -> Option<&Graph> {
        self.graphs.iter().find(|g| g.id == id)
    }

    pub fn graph_mut(&mut self, id: Uuid) -> Option<&mut Graph> {
        self.graphs.iter_mut().find(|g| g.id == id)
    }
}

#[cfg(test)]
mod instance_scope_tests {
    use super::*;
    use crate::workflows::{PortValue, PortValues};

    fn payload_with_camera(v: PortValue) -> PortValues {
        let mut p = PortValues::new();
        p.insert("camera".into(), v);
        p
    }

    #[test]
    fn single_is_one_instance_and_ignores_the_camera() {
        // The default, and the migration's safety: every graph alive today is Single, keys on "",
        // and addresses exactly the state rows it always did.
        let s = InstanceScope::Single;
        assert_eq!(s.key_for(&PortValues::new()), "");
        assert_eq!(s.key_for(&payload_with_camera(PortValue::Text("10.0.0.9".into()))), "");
        assert_eq!(InstanceScope::default(), InstanceScope::Single);
    }

    #[test]
    fn per_camera_separates_cameras() {
        // The whole point: A and B are different instances, so they can't share a window.
        let s = InstanceScope::PerCamera;
        let a = s.key_for(&payload_with_camera(PortValue::Text("cam-a".into())));
        let b = s.key_for(&payload_with_camera(PortValue::Text("cam-b".into())));
        assert_ne!(a, b);
        assert_eq!(a, "cam-a");
    }

    #[test]
    fn per_camera_is_stable_for_the_same_camera() {
        // And the other half: the debounce only survives if a re-trigger lands on the SAME key.
        let s = InstanceScope::PerCamera;
        let first = s.key_for(&payload_with_camera(PortValue::Text("cam-a".into())));
        let again = s.key_for(&payload_with_camera(PortValue::Text("cam-a".into())));
        assert_eq!(first, again, "a re-trigger must find the same instance or the debounce dies");
    }

    #[test]
    fn per_camera_accepts_the_camera_either_side_of_seeding() {
        // The wire carries Text (ip/id); `seed_event_payload` upgrades it to a CameraEntity. A
        // DelayTrigger can arm on either side of that, and both must key the same instance.
        let s = InstanceScope::PerCamera;
        let raw = s.key_for(&payload_with_camera(PortValue::Text("cam-1".into())));
        let typed = s.key_for(&payload_with_camera(PortValue::Camera(CameraEntity {
            id: "cam-1".into(),
            ip: "10.0.0.9".into(),
            name: "Hall".into(),
        })));
        assert_eq!(raw, typed, "Text and CameraEntity must resolve to one instance");
    }

    #[test]
    fn per_camera_without_a_camera_is_one_honest_instance() {
        // An event with no camera (telegram_message, say) in a per-camera graph: one instance,
        // not a broken one. "" is a real key, not a missing value.
        assert_eq!(InstanceScope::PerCamera.key_for(&PortValues::new()), "");
    }

    #[test]
    fn scope_survives_json_and_old_graphs_have_none() {
        let g: Graph = serde_json::from_str(r#"{"id":"00000000-0000-0000-0000-000000000000","name":"old"}"#).unwrap();
        assert_eq!(g.instance_scope, InstanceScope::Single, "a graph saved before instances is Single");
        let mut g2 = Graph::new("g");
        g2.instance_scope = InstanceScope::PerCamera;
        let back: Graph = serde_json::from_str(&serde_json::to_string(&g2).unwrap()).unwrap();
        assert_eq!(back.instance_scope, InstanceScope::PerCamera);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chain() -> (Graph, u32, u32) {
        // get_last_alert.message -> create_alert.message
        let mut g = Graph::new("test");
        let a = g.add_node(NodeKind::GetLastAlert, 0, 0);
        let b = g.add_node(NodeKind::CreateAlert, 30, 0);
        g.add_edge(a, "message", b, "message").unwrap();
        (g, a, b)
    }

    #[test]
    fn add_node_assigns_incrementing_ids_and_default_config() {
        let mut g = Graph::new("g");
        let a = g.add_node(NodeKind::CreateAlert, 1, 2);
        let b = g.add_node(NodeKind::Format, 3, 4);
        assert_eq!((a, b), (1, 2));
        assert_eq!(g.next_id, 3);
        assert_eq!(g.node(a).unwrap().config["source"], "workflow");
    }

    #[test]
    fn remove_node_cascades_edges() {
        let (mut g, a, b) = chain();
        assert_eq!(g.edges.len(), 1);
        g.remove_node(a);
        assert!(g.node(a).is_none());
        assert_eq!(g.edges.len(), 0);
        assert_eq!(g.children(b), Vec::<u32>::new());
    }

    #[test]
    fn children_and_topo_order_follow_edges() {
        let (g, a, b) = chain();
        assert_eq!(g.children(a), vec![b]);
        assert_eq!(g.topo_order().unwrap(), vec![a, b]);
        assert_eq!(g.roots(), vec![a]);
    }

    #[test]
    fn add_edge_rejects_type_mismatch_double_fed_cycle_and_unknown() {
        let mut g = Graph::new("g");
        let snap = g.add_node(NodeKind::CameraSnapshot, 0, 0);
        let fmt = g.add_node(NodeKind::Format, 0, 0);
        assert!(g
            .add_edge(snap, "image", fmt, "p1")
            .unwrap_err()
            .contains("type mismatch"));
        assert!(g.add_edge(fmt, "text", fmt, "p1").is_err()); // self-loop
        assert!(g.add_edge(snap, "nope", fmt, "p1").is_err()); // unknown port

        let gla = g.add_node(NodeKind::GetLastAlert, 0, 0);
        let ca = g.add_node(NodeKind::CreateAlert, 0, 0);
        g.add_edge(gla, "message", ca, "message").unwrap();
        assert!(g
            .add_edge(fmt, "text", ca, "message")
            .unwrap_err()
            .contains("already wired"));

        let a = g.add_node(NodeKind::Format, 0, 0);
        let b = g.add_node(NodeKind::Format, 0, 0);
        g.add_edge(a, "text", b, "p1").unwrap();
        assert!(g
            .add_edge(b, "text", a, "p1")
            .unwrap_err()
            .contains("cycle"));
    }

    #[test]
    fn exec_ports_wire_between_any_nodes_but_not_to_data() {
        let mut g = Graph::new("g");
        let a = g.add_node(NodeKind::GetLastAlert, 0, 0);
        let w = g.add_node(NodeKind::Wait, 0, 0);
        let b = g.add_node(NodeKind::CreateAlert, 0, 0);
        // exec → exec sequences nodes that share no data.
        g.add_edge(a, "exec_out", w, "exec_in").unwrap();
        g.add_edge(w, "exec_out", b, "exec_in").unwrap();
        // exec can't connect to a data port.
        assert!(g
            .add_edge(a, "exec_out", b, "message")
            .unwrap_err()
            .contains("type mismatch"));
        assert!(g
            .add_edge(a, "message", b, "exec_in")
            .unwrap_err()
            .contains("type mismatch"));
        // Wait is control-flow only — it has no data ports.
        assert!(NodeKind::Wait.input_ports().is_empty());
        assert!(NodeKind::Wait.output_ports().is_empty());
    }

    #[test]
    fn if_node_has_true_false_exec_outputs() {
        let mut g = Graph::new("g");
        let gate = g.add_node(NodeKind::If, 0, 0);
        let a = g.add_node(NodeKind::Format, 0, 0);
        // Both branch pins are wireable, typed Exec.
        g.add_edge(gate, "true", a, "exec_in").unwrap();
        let b = g.add_node(NodeKind::Format, 0, 0);
        g.add_edge(gate, "false", b, "exec_in").unwrap();
        let names: Vec<_> = NodeKind::If
            .exec_out_ports()
            .iter()
            .map(|p| p.name)
            .collect();
        assert_eq!(names, vec!["true", "false"]);
        assert_eq!(NodeKind::If.output_port("true").unwrap().ty, PortType::Exec);
        // `condition` is a required Bool data input.
        assert!(NodeKind::If.input_port("condition").unwrap().required);
    }

    #[test]
    fn graph_json_round_trips_with_trigger_concurrency_and_typed_edges() {
        let (mut g, _a, _b) = chain();
        g.trigger = Trigger::Cron { every_secs: 10 };
        g.concurrency = ConcurrencyPolicy::QueueSingle { max_queued: 3 };
        g.enabled = true;
        let s = serde_json::to_string(&g).unwrap();
        let g2: Graph = serde_json::from_str(&s).unwrap();
        assert_eq!(g, g2);
    }

    #[test]
    fn memoize_defaults_false_for_new_nodes_and_round_trips() {
        // New nodes default to memoize=false.
        let mut g = Graph::new("g");
        let n = g.add_node(NodeKind::Format, 0, 0);
        assert!(
            !g.node(n).unwrap().memoize,
            "new node defaults to memoize=false"
        );

        // An old persisted node JSON without the `memoize` key deserializes to true (back-compat).
        let old = r#"{"id":7,"kind":"format","x":0,"y":0,"config":{}}"#;
        let node: GraphNode = serde_json::from_str(old).unwrap();
        assert!(
            node.memoize,
            "missing memoize → true (back-compat for pre-memoize nodes)"
        );

        // memoize=true round-trips.
        g.node_mut(n).unwrap().memoize = true;
        let s = serde_json::to_string(&g).unwrap();
        let g2: Graph = serde_json::from_str(&s).unwrap();
        assert!(g2.node(n).unwrap().memoize);
    }

    #[test]
    fn workflows_collection_create_remove_lookup() {
        let mut w = Workflows::from_graphs(Vec::new());
        let id1 = w.create_graph("alpha");
        let id2 = w.create_graph("beta");
        assert_ne!(id1, id2);
        assert_eq!(w.graphs.len(), 2);
        assert_eq!(w.graph(id2).unwrap().name, "beta");
        w.remove_graph(id1);
        assert_eq!(w.graphs.len(), 1);
        assert!(w.graph(id1).is_none());
    }

    #[test]
    fn slug_round_trips_all_kinds() {
        for k in NodeKind::ALL {
            assert_eq!(NodeKind::from_slug(k.slug()), Some(k));
        }
    }

    #[test]
    fn string_switch_wires_arm_and_default_exec_ports() {
        let mut g = Graph::new("g");
        let sw = g.add_node(NodeKind::StringSwitch, 0, 0);
        g.node_mut(sw).unwrap().config = json!({ "arms": ["red", "blue"] });
        let a = g.add_node(NodeKind::Format, 0, 0);
        let b = g.add_node(NodeKind::Format, 0, 0);
        let d = g.add_node(NodeKind::Format, 0, 0);
        // Named arm ports resolve and wire correctly.
        g.add_edge(sw, "red", a, "exec_in")
            .expect("'red' arm wires");
        g.add_edge(sw, "blue", b, "exec_in")
            .expect("'blue' arm wires");
        // The built-in default arm also wires.
        g.add_edge(sw, "default", d, "exec_in")
            .expect("'default' arm wires");
        // An arm not in config is rejected.
        let c = g.add_node(NodeKind::Format, 0, 0);
        let err = g.add_edge(sw, "green", c, "exec_in").unwrap_err();
        assert!(
            err.contains("no output port"),
            "unknown arm rejected: {err}"
        );
    }

    #[test]
    fn string_switch_value_port_is_required_text() {
        let k = NodeKind::StringSwitch;
        let p = k.input_port("value").expect("value port exists");
        assert_eq!(p.ty, PortType::Text);
        assert!(p.required);
        // exec_out_ports is empty (dynamic); output_ports is empty.
        assert!(k.exec_out_ports().is_empty());
        assert!(k.output_ports().is_empty());
    }

    #[test]
    fn string_switch_default_config_has_two_arms() {
        let cfg = NodeKind::StringSwitch.default_config();
        let arms = cfg["arms"].as_array().expect("arms is an array");
        assert_eq!(arms.len(), 2);
        assert_eq!(arms[0].as_str(), Some("arm_1"));
        assert_eq!(arms[1].as_str(), Some("arm_2"));
    }
}
