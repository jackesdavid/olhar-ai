//! Workflows domain — typed ports, nodes, edges, named graphs, persistence.
//! Ported from the ratatui `src/workflows/domain.rs` (spec 013) for the web app;
//! the ratatui editor state (`WfMode`/`WorkflowsState`/viewport) is dropped — the
//! Svelte editor owns view state. Persistence is path-parameterized → app-data
//! `workflows.json`.
//!
//! Isolation model: a node declares typed input/output **ports**; **edges** map a
//! source output port to a target input port. The engine owns all edges and
//! resolves wiring — a node never references another node.

pub use crate::alerts::AlertLevel;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

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
    /// Wildcard — compatible with ANY type at wire time. Used by `ForEach` (`items`/`item`)
    /// so one loop node handles every array/element type. See [`Graph::compatible`].
    Any,
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
        ResolvedPort { name: p.name.to_string(), ty: p.ty, required: p.required }
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
                    Some(ResolvedPort { name, ty, required: false })
                })
                .collect()
        })
        .unwrap_or_default()
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
    /// Destructure a `PersonDetection` element → `bbox` + `confidence` (like Unreal's Break).
    BreakPersonDetection,
    /// Destructure a `FaceDetection` element → `bbox` + `confidence` + `landmarks`.
    BreakFaceDetection,
    /// Vision: match a face in an image against the enrolled people (pgvector kNN).
    RecognizePerson,
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
    /// Flow control: rate-limit gate — lets exec through only when at least `window_secs`
    /// have elapsed since the node last fired. Persists `last_fired_at` across runs via
    /// node state. Outputs `elapsed_secs` (how long since last fire, or window+1 on first
    /// run) and `ok` (true = gate opened). Engine-driven (not `run_node`).
    Cooldown,
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

const RECORD_PRESENCE_IN: [Port; 4] = [
    p("room", PortType::Text, false),
    p("category", PortType::Text, false),
    p("label", PortType::Text, false),
    p("camera", PortType::Text, false),
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

const CAMERA_SNAPSHOT_IN: [Port; 1] = [p("camera", PortType::Text, true)];
const CAMERA_SNAPSHOT_OUT: [Port; 3] = [
    p("image", PortType::Image, false),
    p("result", PortType::Text, false),
    p("ok", PortType::Bool, false),
];

const GET_CURRENT_FRAME_IN: [Port; 1] = [p("camera", PortType::Text, true)];
const GET_CURRENT_FRAME_OUT: [Port; 3] = [
    p("image", PortType::Image, false),
    p("age_ms", PortType::Num, false), // how old the returned frame is (0 = fresh capture)
    p("ok", PortType::Bool, false),
];

const DESCRIBE_IMAGE_IN: [Port; 1] = [p("image", PortType::Image, true)];
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

// If: a Bool condition routes the run down one of two exec branches.
const IF_IN: [Port; 1] = [p("condition", PortType::Bool, true)];

// For each: iterate ANY array (a wired `*Array`, or a text list authored in config).
// `items`/`item` are `Any` so one loop node handles every element type.
const FOR_EACH_IN: [Port; 1] = [p("items", PortType::Any, false)];
const FOR_EACH_OUT: [Port; 2] =
    [p("item", PortType::Any, false), p("index", PortType::Num, false)];

// Break (destructure) nodes — split a struct element into its fields, like Unreal's Break.
const BREAK_PERSON_IN: [Port; 1] = [p("person", PortType::Person, true)];
const BREAK_PERSON_OUT: [Port; 2] =
    [p("bbox", PortType::NumArray, false), p("confidence", PortType::Num, false)];
const BREAK_FACE_IN: [Port; 1] = [p("face", PortType::Face, true)];
const BREAK_FACE_OUT: [Port; 3] = [
    p("bbox", PortType::NumArray, false),
    p("confidence", PortType::Num, false),
    p("landmarks", PortType::Num, false), // count of the 5-point landmark set
];

// Motion detection: compare two frames; report whether/how much they differ.
const MOTION_IN: [Port; 2] =
    [p("image_a", PortType::Image, true), p("image_b", PortType::Image, true)];
const MOTION_OUT: [Port; 3] = [
    p("moved", PortType::Bool, false),
    p("score", PortType::Num, false),
    p("ok", PortType::Bool, false),
];
const MOTION_WATCH_IN: [Port; 1] = [p("camera", PortType::Text, true)];

// Person detection: an image in → whether a person is present (via the AI sidecar).
const PERSON_DETECTION_IN: [Port; 1] = [p("image", PortType::Image, true)];
const PERSON_DETECTION_OUT: [Port; 4] = [
    p("count", PortType::Num, false),
    p("confidence", PortType::Num, false), // percent 0..100 (top person)
    p("persons", PortType::PersonArray, false), // one PersonDetection per person — loop this
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

// Print: a message to echo to the Console (+ optional on-screen flash).
const PRINT_IN: [Port; 2] =
    [p("message", PortType::Text, false), p("on_screen", PortType::Bool, false)];

// Show image: a debug sink — an image in, flashed on the canvas. No data out.
const SHOW_IMAGE_IN: [Port; 1] = [p("image", PortType::Image, true)];

// Flip Flop: exec a/b outputs (alternating), plus is_a bool data output.
const EXEC_OUT_FLIP_FLOP: [Port; 2] = [
    p("a", PortType::Exec, false),
    p("b", PortType::Exec, false),
];
const FLIP_FLOP_OUT: [Port; 1] = [p("is_a", PortType::Bool, false)];

// Compare: two numeric inputs (wired or literal) → a bool result.
const COMPARE_IN: [Port; 2] = [p("a", PortType::Num, false), p("b", PortType::Num, false)];
const COMPARE_OUT: [Port; 1] = [p("result", PortType::Bool, false)];

const COOLDOWN_OUT: [Port; 2] = [
    p("elapsed_secs", PortType::Num, false),
    p("ok", PortType::Bool, false),
];

const NO_PORTS: [Port; 0] = [];

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
    pub const ALL: [NodeKind; 27] = [
        NodeKind::GetLastAlert,
        NodeKind::GetRecipient,
        NodeKind::CreateAlert,
        NodeKind::DeliverAlert,
        NodeKind::CameraSnapshot,
        NodeKind::GetCurrentFrame,
        NodeKind::DescribeImage,
        NodeKind::NotifyRecipient,
        NodeKind::Format,
        NodeKind::Wait,
        NodeKind::HttpPost,
        NodeKind::If,
        NodeKind::ForEach,
        NodeKind::FlipFlop,
        NodeKind::Compare,
        NodeKind::MotionDetection,
        NodeKind::MotionWatch,
        NodeKind::PersonDetection,
        NodeKind::BreakPersonDetection,
        NodeKind::BreakFaceDetection,
        NodeKind::RecognizePerson,
        NodeKind::RecordPresence,
        NodeKind::EmitEvent,
        NodeKind::OnEvent,
        NodeKind::Print,
        NodeKind::ShowImage,
        NodeKind::Cooldown,
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
            NodeKind::GetCurrentFrame => "get_current_frame",
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
            NodeKind::BreakPersonDetection => "break_person_detection",
            NodeKind::BreakFaceDetection => "break_face_detection",
            NodeKind::RecognizePerson => "recognize_person",
            NodeKind::RecordPresence => "record_presence",
            NodeKind::EmitEvent => "emit_event",
            NodeKind::OnEvent => "on_event",
            NodeKind::Print => "print",
            NodeKind::ShowImage => "show_image",
            NodeKind::FlipFlop => "flip_flop",
            NodeKind::Compare => "compare",
            NodeKind::Cooldown => "cooldown",
        }
    }

    pub fn from_slug(s: &str) -> Option<NodeKind> {
        NodeKind::ALL.into_iter().find(|k| k.slug() == s)
    }

    pub fn label(self) -> &'static str {
        match self {
            NodeKind::GetLastAlert => "Get last alert",
            NodeKind::GetRecipient => "Get recipient",
            NodeKind::CreateAlert => "Create alert",
            NodeKind::DeliverAlert => "Deliver alert",
            NodeKind::CameraSnapshot => "Camera snapshot",
            NodeKind::GetCurrentFrame => "Get current frame",
            NodeKind::DescribeImage => "Describe image",
            NodeKind::NotifyRecipient => "Notify recipient",
            NodeKind::Format => "Format",
            NodeKind::Wait => "Wait",
            NodeKind::HttpPost => "HTTP post",
            NodeKind::If => "If",
            NodeKind::ForEach => "For each",
            NodeKind::MotionDetection => "Motion detection",
            NodeKind::MotionWatch => "Motion watch",
            NodeKind::PersonDetection => "Person detection",
            NodeKind::BreakPersonDetection => "Break person detection",
            NodeKind::BreakFaceDetection => "Break face detection",
            NodeKind::RecognizePerson => "Recognize person",
            NodeKind::RecordPresence => "Record presence",
            NodeKind::EmitEvent => "Emit event",
            NodeKind::OnEvent => "On event",
            NodeKind::Print => "Print",
            NodeKind::ShowImage => "Show image",
            NodeKind::FlipFlop => "Flip flop",
            NodeKind::Compare => "Compare",
            NodeKind::Cooldown => "Cooldown",
        }
    }

    /// The module this node belongs to — used to group the editor's node palette.
    pub fn category(self) -> &'static str {
        match self {
            NodeKind::GetLastAlert | NodeKind::CreateAlert | NodeKind::DeliverAlert => "Alerts",
            NodeKind::GetRecipient | NodeKind::NotifyRecipient => "Recipients",
            NodeKind::CameraSnapshot | NodeKind::GetCurrentFrame => "Cameras",
            NodeKind::DescribeImage => "AI",
            NodeKind::Format => "Text",
            NodeKind::Wait
            | NodeKind::If
            | NodeKind::ForEach
            | NodeKind::FlipFlop
            | NodeKind::Compare
            | NodeKind::Cooldown
            | NodeKind::Print
            | NodeKind::ShowImage
            | NodeKind::EmitEvent
            | NodeKind::OnEvent => "Flow",
            NodeKind::HttpPost => "Network",
            NodeKind::MotionDetection
            | NodeKind::MotionWatch
            | NodeKind::PersonDetection
            | NodeKind::RecognizePerson
            | NodeKind::BreakPersonDetection
            | NodeKind::BreakFaceDetection => "Vision",
            NodeKind::RecordPresence => "Presence",
        }
    }

    /// Exec OUTPUT pins for this kind. Default is a single `exec_out`; `If` splits
    /// the run into `true`/`false` branches. (`exec_in` is universal — see EXEC_IN.)
    pub fn exec_out_ports(self) -> &'static [Port] {
        match self {
            NodeKind::If => &EXEC_OUT_IF,
            NodeKind::ForEach => &EXEC_OUT_FOR_EACH,
            NodeKind::FlipFlop => &EXEC_OUT_FLIP_FLOP,
            _ => &EXEC_OUT_DEFAULT,
        }
    }

    pub fn input_ports(self) -> &'static [Port] {
        match self {
            NodeKind::GetLastAlert => &NO_PORTS,
            NodeKind::GetRecipient => &NO_PORTS,
            NodeKind::CreateAlert => &CREATE_ALERT_IN,
            NodeKind::DeliverAlert => &DELIVER_ALERT_IN,
            NodeKind::CameraSnapshot => &CAMERA_SNAPSHOT_IN,
            NodeKind::GetCurrentFrame => &GET_CURRENT_FRAME_IN,
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
            NodeKind::BreakPersonDetection => &BREAK_PERSON_IN,
            NodeKind::BreakFaceDetection => &BREAK_FACE_IN,
            NodeKind::RecognizePerson => &RECOGNIZE_PERSON_IN,
            NodeKind::RecordPresence => &RECORD_PRESENCE_IN,
            NodeKind::EmitEvent => &NO_PORTS, // exec-only (universal exec_in/out); event name in config
            NodeKind::OnEvent => &NO_PORTS,   // entry: only an exec_out; activated by the event
            NodeKind::Print => &PRINT_IN,
            NodeKind::ShowImage => &SHOW_IMAGE_IN,
            NodeKind::FlipFlop => &NO_PORTS, // engine-driven; no data inputs
            NodeKind::Compare => &COMPARE_IN,
            NodeKind::Cooldown => &NO_PORTS, // engine-driven; window_secs is config
        }
    }

    pub fn output_ports(self) -> &'static [Port] {
        match self {
            NodeKind::GetLastAlert => &GET_LAST_ALERT_OUT,
            NodeKind::GetRecipient => &GET_RECIPIENT_OUT,
            NodeKind::CreateAlert => &CREATE_ALERT_OUT,
            NodeKind::DeliverAlert => &DELIVER_ALERT_OUT,
            NodeKind::CameraSnapshot => &CAMERA_SNAPSHOT_OUT,
            NodeKind::GetCurrentFrame => &GET_CURRENT_FRAME_OUT,
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
            NodeKind::BreakPersonDetection => &BREAK_PERSON_OUT,
            NodeKind::BreakFaceDetection => &BREAK_FACE_OUT,
            NodeKind::RecognizePerson => &RECOGNIZE_PERSON_OUT,
            NodeKind::RecordPresence => &RECORD_PRESENCE_OUT,
            NodeKind::EmitEvent => &NO_PORTS, // exec passes through, no data out
            NodeKind::OnEvent => &NO_PORTS,   // drives its subtree via exec_out only
            NodeKind::Print => &NO_PORTS, // a sink — exec passes through, no data out
            NodeKind::ShowImage => &NO_PORTS, // a sink — exec passes through, no data out
            NodeKind::FlipFlop => &FLIP_FLOP_OUT,
            NodeKind::Compare => &COMPARE_OUT,
            NodeKind::Cooldown => &COOLDOWN_OUT,
        }
    }

    pub fn input_port(self, name: &str) -> Option<Port> {
        if name == EXEC_IN.name {
            return Some(EXEC_IN);
        }
        self.input_ports().iter().copied().find(|p| p.name == name)
    }

    pub fn output_port(self, name: &str) -> Option<Port> {
        if let Some(e) = self.exec_out_ports().iter().copied().find(|p| p.name == name) {
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
            NodeKind::GetCurrentFrame => json!({ "camera": "first", "max_age_ms": "2000", "timeout_secs": "10" }),
            NodeKind::DescribeImage => json!({ "timeout_secs": "65" }),
            NodeKind::NotifyRecipient => json!({ "message": "", "timeout_secs": "35" }),
            NodeKind::Format => json!({ "template": "{p1} — {p2}", "p1": "", "p2": "", "p3": "" }),
            NodeKind::Wait => json!({ "seconds": "1" }),
            NodeKind::HttpPost => json!({ "url": "", "headers": {}, "fields": {}, "file": "", "timeout_secs": "35" }),
            NodeKind::If => json!({}),
            NodeKind::ForEach => json!({ "items": "" }),
            NodeKind::MotionDetection => json!({ "sensitivity": "2", "pixel_threshold": "25", "timeout_secs": "30" }),
            NodeKind::MotionWatch => json!({ "camera": "first", "sensitivity": "2", "pixel_threshold": "25", "sample_gap_ms": "400", "timeout_secs": "15" }),
            NodeKind::PersonDetection => json!({ "timeout_secs": "30" }),
            NodeKind::BreakPersonDetection => json!({}),
            NodeKind::BreakFaceDetection => json!({}),
            NodeKind::RecognizePerson => json!({ "max_distance": "0.4", "timeout_secs": "30" }),
            NodeKind::RecordPresence => json!({ "category": "person", "room": "", "label": "", "duration_secs": "8" }),
            NodeKind::EmitEvent => json!({ "event": "" }),
            NodeKind::OnEvent => json!({ "event": "" }),
            NodeKind::Print => json!({ "message": "", "on_screen": true, "level": "info" }),
            NodeKind::ShowImage => json!({ "label": "" }),
            NodeKind::FlipFlop => json!({}),
            NodeKind::Compare => json!({ "operator": "==", "a": "0", "b": "0" }),
            NodeKind::Cooldown => json!({ "window_secs": "60" }),
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
            k => k.input_ports().iter().map(ResolvedPort::from).collect(),
        }
    }

    /// This node's **effective** output ports. Static for every kind except `OnEvent`,
    /// whose outputs are one-per-payload-field derived from its config.
    pub fn output_ports(&self) -> Vec<ResolvedPort> {
        match self.kind {
            NodeKind::OnEvent => payload_ports(&self.config),
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
        if let Some(e) = self.kind.exec_out_ports().iter().find(|p| p.name == name) {
            return Some(ResolvedPort::from(e));
        }
        self.output_ports().into_iter().find(|p| p.name == name)
    }
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
}

impl TriggerSource {
    pub fn as_str(self) -> &'static str {
        match self {
            TriggerSource::Manual => "manual",
            TriggerSource::Cron => "cron",
            TriggerSource::Event => "event",
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
    Graph { id: u32, name: String },
    System { label: String },
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
        Viewport { x: 40.0, y: 40.0, zoom: 1.0 }
    }
}

/// One named automation: a trigger + a DAG of typed nodes/edges.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Graph {
    pub id: u32,
    pub name: String,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub trigger: Trigger,
    #[serde(default)]
    pub concurrency: ConcurrencyPolicy,
    /// Run nodes that have no edges (orphans on the canvas). Default `false` —
    /// orphans are skipped so leftover/experimental nodes don't fire.
    #[serde(default)]
    pub run_orphans: bool,
    /// Emit a per-node debug log line during a run (for the editor console).
    #[serde(default)]
    pub debug: bool,
    #[serde(default = "one")]
    pub next_id: u32,
    #[serde(default)]
    pub nodes: Vec<GraphNode>,
    #[serde(default)]
    pub edges: Vec<Edge>,
    #[serde(default)]
    pub viewport: Viewport,
}

fn one() -> u32 {
    1
}

impl Graph {
    pub fn new(id: u32, name: &str) -> Self {
        Graph {
            id,
            name: name.to_string(),
            enabled: false,
            trigger: Trigger::Manual,
            concurrency: ConcurrencyPolicy::SkipIfRunning,
            run_orphans: false,
            debug: false,
            next_id: 1,
            nodes: Vec::new(),
            edges: Vec::new(),
            viewport: Viewport::default(),
        }
    }

    pub fn node(&self, id: u32) -> Option<&GraphNode> {
        self.nodes.iter().find(|n| n.id == id)
    }

    pub fn node_mut(&mut self, id: u32) -> Option<&mut GraphNode> {
        self.nodes.iter_mut().find(|n| n.id == id)
    }

    /// Add a node at (x, y); returns its new id. Event entry/emit nodes never memoize
    /// (they're no-op signal nodes — caching is meaningless and confusing), so default off.
    pub fn add_node(&mut self, kind: NodeKind, x: i32, y: i32) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        let memoize = !matches!(kind, NodeKind::EmitEvent | NodeKind::OnEvent);
        self.nodes.push(GraphNode { id, kind, x, y, config: kind.default_config(), memoize });
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
        if self.edges.iter().any(|e| e.to == to && e.to_port == to_port) {
            return Err(format!("input '{to_port}' is already wired"));
        }
        // Cycles are allowed ONLY through an **exec** back-edge (a loop): the engine runs
        // the forward graph in order and re-enters the loop along the back-edge, bounded by
        // a step/time budget. A **data** cycle has no defined value (the input would depend
        // on an output not yet produced), so it's still rejected.
        let is_exec = to_port == EXEC_IN.name;
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
        let mut adj: HashMap<u32, Vec<u32>> = self.nodes.iter().map(|n| (n.id, Vec::new())).collect();
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
    pub next_graph_id: u32,
}

impl Workflows {
    pub fn from_graphs(graphs: Vec<Graph>) -> Self {
        let next_graph_id = graphs.iter().map(|g| g.id).max().map_or(1, |m| m + 1);
        Workflows { graphs, next_graph_id }
    }

    pub fn create_graph(&mut self, name: &str) -> u32 {
        let id = self.next_graph_id;
        self.next_graph_id += 1;
        self.graphs.push(Graph::new(id, name));
        id
    }

    pub fn remove_graph(&mut self, id: u32) {
        self.graphs.retain(|g| g.id != id);
    }

    pub fn graph(&self, id: u32) -> Option<&Graph> {
        self.graphs.iter().find(|g| g.id == id)
    }

    pub fn graph_mut(&mut self, id: u32) -> Option<&mut Graph> {
        self.graphs.iter_mut().find(|g| g.id == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chain() -> (Graph, u32, u32) {
        // get_last_alert.message -> create_alert.message
        let mut g = Graph::new(1, "test");
        let a = g.add_node(NodeKind::GetLastAlert, 0, 0);
        let b = g.add_node(NodeKind::CreateAlert, 30, 0);
        g.add_edge(a, "message", b, "message").unwrap();
        (g, a, b)
    }

    #[test]
    fn add_node_assigns_incrementing_ids_and_default_config() {
        let mut g = Graph::new(1, "g");
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
        let mut g = Graph::new(1, "g");
        let snap = g.add_node(NodeKind::CameraSnapshot, 0, 0);
        let fmt = g.add_node(NodeKind::Format, 0, 0);
        assert!(g.add_edge(snap, "image", fmt, "p1").unwrap_err().contains("type mismatch"));
        assert!(g.add_edge(fmt, "text", fmt, "p1").is_err()); // self-loop
        assert!(g.add_edge(snap, "nope", fmt, "p1").is_err()); // unknown port

        let gla = g.add_node(NodeKind::GetLastAlert, 0, 0);
        let ca = g.add_node(NodeKind::CreateAlert, 0, 0);
        g.add_edge(gla, "message", ca, "message").unwrap();
        assert!(g.add_edge(fmt, "text", ca, "message").unwrap_err().contains("already wired"));

        let a = g.add_node(NodeKind::Format, 0, 0);
        let b = g.add_node(NodeKind::Format, 0, 0);
        g.add_edge(a, "text", b, "p1").unwrap();
        assert!(g.add_edge(b, "text", a, "p1").unwrap_err().contains("cycle"));
    }

    #[test]
    fn exec_ports_wire_between_any_nodes_but_not_to_data() {
        let mut g = Graph::new(1, "g");
        let a = g.add_node(NodeKind::GetLastAlert, 0, 0);
        let w = g.add_node(NodeKind::Wait, 0, 0);
        let b = g.add_node(NodeKind::CreateAlert, 0, 0);
        // exec → exec sequences nodes that share no data.
        g.add_edge(a, "exec_out", w, "exec_in").unwrap();
        g.add_edge(w, "exec_out", b, "exec_in").unwrap();
        // exec can't connect to a data port.
        assert!(g.add_edge(a, "exec_out", b, "message").unwrap_err().contains("type mismatch"));
        assert!(g.add_edge(a, "message", b, "exec_in").unwrap_err().contains("type mismatch"));
        // Wait is control-flow only — it has no data ports.
        assert!(NodeKind::Wait.input_ports().is_empty());
        assert!(NodeKind::Wait.output_ports().is_empty());
    }

    #[test]
    fn if_node_has_true_false_exec_outputs() {
        let mut g = Graph::new(1, "g");
        let gate = g.add_node(NodeKind::If, 0, 0);
        let a = g.add_node(NodeKind::Format, 0, 0);
        // Both branch pins are wireable, typed Exec.
        g.add_edge(gate, "true", a, "exec_in").unwrap();
        let b = g.add_node(NodeKind::Format, 0, 0);
        g.add_edge(gate, "false", b, "exec_in").unwrap();
        let names: Vec<_> = NodeKind::If.exec_out_ports().iter().map(|p| p.name).collect();
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
    fn memoize_defaults_true_for_old_blobs_and_round_trips() {
        // New nodes default to memoize=true.
        let mut g = Graph::new(1, "g");
        let n = g.add_node(NodeKind::Format, 0, 0);
        assert!(g.node(n).unwrap().memoize, "new node defaults to memoize=true");

        // An old persisted node JSON without the `memoize` key deserializes to true.
        let old = r#"{"id":7,"kind":"format","x":0,"y":0,"config":{}}"#;
        let node: GraphNode = serde_json::from_str(old).unwrap();
        assert!(node.memoize, "missing memoize → true (back-compat)");

        // memoize=false round-trips.
        g.node_mut(n).unwrap().memoize = false;
        let s = serde_json::to_string(&g).unwrap();
        let g2: Graph = serde_json::from_str(&s).unwrap();
        assert!(!g2.node(n).unwrap().memoize);
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
}
