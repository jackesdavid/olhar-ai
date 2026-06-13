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
#[serde(rename_all = "lowercase")]
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
    /// Vision: compare two frames and report whether/how much they differ.
    MotionDetection,
    /// Vision: detect whether a person is present in an image (via the AI sidecar).
    PersonDetection,
    /// Vision: match a face in an image against the enrolled people (pgvector kNN).
    RecognizePerson,
    /// Debug: print a `message` to the editor Console (always) and, unless `on_screen`
    /// is off, flash it as on-screen text over the canvas — like Unreal's Print String.
    Print,
    /// Debug: show an `image` on the editor canvas — a sink for eyeballing a frame as
    /// it flows through the graph. Like `Print`, but for pixels; emits no data.
    ShowImage,
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

// Motion detection: compare two frames; report whether/how much they differ.
const MOTION_IN: [Port; 2] =
    [p("image_a", PortType::Image, true), p("image_b", PortType::Image, true)];
const MOTION_OUT: [Port; 3] = [
    p("moved", PortType::Bool, false),
    p("score", PortType::Num, false),
    p("ok", PortType::Bool, false),
];

// Person detection: an image in → whether a person is present (via the AI sidecar).
const PERSON_DETECTION_IN: [Port; 1] = [p("image", PortType::Image, true)];
const PERSON_DETECTION_OUT: [Port; 4] = [
    p("person", PortType::Bool, false),
    p("count", PortType::Num, false),
    p("confidence", PortType::Num, false), // percent 0..100
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

const NO_PORTS: [Port; 0] = [];

// Universal control-flow ports — every node has an `exec_in` and, by default, an
// `exec_out`. They carry no value; they sequence/gate execution. A node kind may
// override its exec OUTPUTS (e.g. `If` splits into `true`/`false`).
pub const EXEC_IN: Port = p("exec_in", PortType::Exec, false);
pub const EXEC_OUT: Port = p("exec_out", PortType::Exec, false);
pub const EXEC_TRUE: Port = p("true", PortType::Exec, false);
pub const EXEC_FALSE: Port = p("false", PortType::Exec, false);
const EXEC_OUT_DEFAULT: [Port; 1] = [EXEC_OUT];
const EXEC_OUT_IF: [Port; 2] = [EXEC_TRUE, EXEC_FALSE];

impl NodeKind {
    pub const ALL: [NodeKind; 16] = [
        NodeKind::GetLastAlert,
        NodeKind::GetRecipient,
        NodeKind::CreateAlert,
        NodeKind::DeliverAlert,
        NodeKind::CameraSnapshot,
        NodeKind::DescribeImage,
        NodeKind::NotifyRecipient,
        NodeKind::Format,
        NodeKind::Wait,
        NodeKind::HttpPost,
        NodeKind::If,
        NodeKind::MotionDetection,
        NodeKind::PersonDetection,
        NodeKind::RecognizePerson,
        NodeKind::Print,
        NodeKind::ShowImage,
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
            NodeKind::DescribeImage => "describe_image",
            NodeKind::NotifyRecipient => "notify_recipient",
            NodeKind::Format => "format",
            NodeKind::Wait => "wait",
            NodeKind::HttpPost => "http_post",
            NodeKind::If => "if",
            NodeKind::MotionDetection => "motion_detection",
            NodeKind::PersonDetection => "person_detection",
            NodeKind::RecognizePerson => "recognize_person",
            NodeKind::Print => "print",
            NodeKind::ShowImage => "show_image",
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
            NodeKind::DescribeImage => "Describe image",
            NodeKind::NotifyRecipient => "Notify recipient",
            NodeKind::Format => "Format",
            NodeKind::Wait => "Wait",
            NodeKind::HttpPost => "HTTP post",
            NodeKind::If => "If",
            NodeKind::MotionDetection => "Motion detection",
            NodeKind::PersonDetection => "Person detection",
            NodeKind::RecognizePerson => "Recognize person",
            NodeKind::Print => "Print",
            NodeKind::ShowImage => "Show image",
        }
    }

    /// The module this node belongs to — used to group the editor's node palette.
    pub fn category(self) -> &'static str {
        match self {
            NodeKind::GetLastAlert | NodeKind::CreateAlert | NodeKind::DeliverAlert => "Alerts",
            NodeKind::GetRecipient | NodeKind::NotifyRecipient => "Recipients",
            NodeKind::CameraSnapshot => "Cameras",
            NodeKind::DescribeImage => "AI",
            NodeKind::Format => "Text",
            NodeKind::Wait | NodeKind::If | NodeKind::Print | NodeKind::ShowImage => "Flow",
            NodeKind::HttpPost => "Network",
            NodeKind::MotionDetection | NodeKind::PersonDetection | NodeKind::RecognizePerson => {
                "Vision"
            }
        }
    }

    /// Exec OUTPUT pins for this kind. Default is a single `exec_out`; `If` splits
    /// the run into `true`/`false` branches. (`exec_in` is universal — see EXEC_IN.)
    pub fn exec_out_ports(self) -> &'static [Port] {
        match self {
            NodeKind::If => &EXEC_OUT_IF,
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
            NodeKind::DescribeImage => &DESCRIBE_IMAGE_IN,
            NodeKind::NotifyRecipient => &NOTIFY_RECIPIENT_IN,
            NodeKind::Format => &FORMAT_IN,
            NodeKind::Wait => &NO_PORTS, // control-flow only — sequenced via exec ports
            NodeKind::HttpPost => &HTTP_POST_IN,
            NodeKind::If => &IF_IN,
            NodeKind::MotionDetection => &MOTION_IN,
            NodeKind::PersonDetection => &PERSON_DETECTION_IN,
            NodeKind::RecognizePerson => &RECOGNIZE_PERSON_IN,
            NodeKind::Print => &PRINT_IN,
            NodeKind::ShowImage => &SHOW_IMAGE_IN,
        }
    }

    pub fn output_ports(self) -> &'static [Port] {
        match self {
            NodeKind::GetLastAlert => &GET_LAST_ALERT_OUT,
            NodeKind::GetRecipient => &GET_RECIPIENT_OUT,
            NodeKind::CreateAlert => &CREATE_ALERT_OUT,
            NodeKind::DeliverAlert => &DELIVER_ALERT_OUT,
            NodeKind::CameraSnapshot => &CAMERA_SNAPSHOT_OUT,
            NodeKind::DescribeImage => &DESCRIBE_IMAGE_OUT,
            NodeKind::NotifyRecipient => &NOTIFY_RECIPIENT_OUT,
            NodeKind::Format => &FORMAT_OUT,
            NodeKind::Wait => &NO_PORTS, // control-flow only — sequenced via exec ports
            NodeKind::HttpPost => &HTTP_POST_OUT,
            NodeKind::If => &NO_PORTS, // outputs are exec branches (true/false)
            NodeKind::MotionDetection => &MOTION_OUT,
            NodeKind::PersonDetection => &PERSON_DETECTION_OUT,
            NodeKind::RecognizePerson => &RECOGNIZE_PERSON_OUT,
            NodeKind::Print => &NO_PORTS, // a sink — exec passes through, no data out
            NodeKind::ShowImage => &NO_PORTS, // a sink — exec passes through, no data out
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
                json!({ "message": "", "room": "", "level": "info", "source": "workflow" })
            }
            NodeKind::DeliverAlert => json!({ "id": "" }),
            NodeKind::CameraSnapshot => json!({ "camera": "first" }),
            NodeKind::DescribeImage => json!({}),
            NodeKind::NotifyRecipient => json!({ "message": "" }),
            NodeKind::Format => json!({ "template": "{p1} — {p2}", "p1": "", "p2": "", "p3": "" }),
            NodeKind::Wait => json!({ "seconds": "1" }),
            NodeKind::HttpPost => json!({ "url": "", "headers": {}, "fields": {}, "file": "" }),
            NodeKind::If => json!({}),
            NodeKind::MotionDetection => json!({ "sensitivity": "2", "pixel_threshold": "25" }),
            NodeKind::PersonDetection => json!({}),
            NodeKind::RecognizePerson => json!({ "max_distance": "0.4" }),
            NodeKind::Print => json!({ "message": "", "on_screen": true, "level": "info" }),
            NodeKind::ShowImage => json!({ "label": "" }),
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Trigger {
    /// Only fired by Run-now.
    Manual,
    /// Engine poller starts a fresh one-shot run every `every_secs`.
    Cron { every_secs: u64 },
    /// Engine starts one run per new alert at or above `min_level`.
    OnNewAlert { min_level: AlertLevel },
}

impl Default for Trigger {
    fn default() -> Self {
        Trigger::Manual
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

    /// Add a node at (x, y); returns its new id.
    pub fn add_node(&mut self, kind: NodeKind, x: i32, y: i32) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        self.nodes.push(GraphNode { id, kind, x, y, config: kind.default_config() });
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
        let fk = self.node(from).ok_or("source node not found")?.kind;
        let tk = self.node(to).ok_or("target node not found")?.kind;
        let ft = fk
            .output_port(from_port)
            .ok_or_else(|| format!("{} has no output port '{from_port}'", fk.label()))?
            .ty;
        let tt = tk
            .input_port(to_port)
            .ok_or_else(|| format!("{} has no input port '{to_port}'", tk.label()))?
            .ty;
        if ft != tt {
            return Err(format!("type mismatch: {ft:?} → {tt:?}"));
        }
        if self.edges.iter().any(|e| e.to == to && e.to_port == to_port) {
            return Err(format!("input '{to_port}' is already wired"));
        }
        if self.reaches(to, from) {
            return Err("that link would create a cycle".into());
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
