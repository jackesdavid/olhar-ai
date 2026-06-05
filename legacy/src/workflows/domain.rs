//! Workflows domain — typed ports, nodes, edges, named graphs, persistence.
//!
//! Isolation model: a node declares typed input/output **ports**; **edges** map a
//! source output port to a target input port. The engine owns all edges and resolves
//! wiring — a node never references another node. See `architecture.md` (spec 013).
#![allow(dead_code)]

pub use crate::alerts::AlertLevel;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

/// Node box size on the canvas grid (y-down, like `MapRoom`). Kept compact so
/// many nodes fit on screen (use `fit()` to frame them all).
pub const NODE_W: i32 = 12;
pub const NODE_H: i32 = 3;

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
    CreateAlert,
    DeliverAlert,
    CameraSnapshot,
    DescribeImage,
    NotifyBeacon,
    /// Pure text composition: substitutes `{p1..p3}` into a `template`.
    Format,
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

const NOTIFY_BEACON_IN: [Port; 3] = [
    p("message", PortType::Text, true),
    p("target", PortType::Text, false),
    p("image", PortType::Image, false),
];
const NOTIFY_BEACON_OUT: [Port; 2] = [
    p("result", PortType::Text, false),
    p("ok", PortType::Bool, false),
];

const FORMAT_IN: [Port; 3] = [
    p("p1", PortType::Text, false),
    p("p2", PortType::Text, false),
    p("p3", PortType::Text, false),
];
const FORMAT_OUT: [Port; 1] = [p("text", PortType::Text, false)];

const NO_PORTS: [Port; 0] = [];

impl NodeKind {
    pub const ALL: [NodeKind; 7] = [
        NodeKind::GetLastAlert,
        NodeKind::CreateAlert,
        NodeKind::DeliverAlert,
        NodeKind::CameraSnapshot,
        NodeKind::DescribeImage,
        NodeKind::NotifyBeacon,
        NodeKind::Format,
    ];

    pub fn label(self) -> &'static str {
        match self {
            NodeKind::GetLastAlert => "GET LAST ALERT",
            NodeKind::CreateAlert => "CREATE ALERT",
            NodeKind::DeliverAlert => "DELIVER ALERT",
            NodeKind::CameraSnapshot => "CAMERA SNAPSHOT",
            NodeKind::DescribeImage => "DESCRIBE IMAGE",
            NodeKind::NotifyBeacon => "NOTIFY BEACON",
            NodeKind::Format => "FORMAT",
        }
    }

    pub fn input_ports(self) -> &'static [Port] {
        match self {
            NodeKind::GetLastAlert => &NO_PORTS,
            NodeKind::CreateAlert => &CREATE_ALERT_IN,
            NodeKind::DeliverAlert => &DELIVER_ALERT_IN,
            NodeKind::CameraSnapshot => &CAMERA_SNAPSHOT_IN,
            NodeKind::DescribeImage => &DESCRIBE_IMAGE_IN,
            NodeKind::NotifyBeacon => &NOTIFY_BEACON_IN,
            NodeKind::Format => &FORMAT_IN,
        }
    }

    pub fn output_ports(self) -> &'static [Port] {
        match self {
            NodeKind::GetLastAlert => &GET_LAST_ALERT_OUT,
            NodeKind::CreateAlert => &CREATE_ALERT_OUT,
            NodeKind::DeliverAlert => &DELIVER_ALERT_OUT,
            NodeKind::CameraSnapshot => &CAMERA_SNAPSHOT_OUT,
            NodeKind::DescribeImage => &DESCRIBE_IMAGE_OUT,
            NodeKind::NotifyBeacon => &NOTIFY_BEACON_OUT,
            NodeKind::Format => &FORMAT_OUT,
        }
    }

    pub fn input_port(self, name: &str) -> Option<Port> {
        self.input_ports().iter().copied().find(|p| p.name == name)
    }

    pub fn output_port(self, name: &str) -> Option<Port> {
        self.output_ports().iter().copied().find(|p| p.name == name)
    }

    /// Static config + literal fallbacks for unconnected input ports.
    pub fn default_config(self) -> Value {
        match self {
            NodeKind::GetLastAlert => json!({}),
            NodeKind::CreateAlert => {
                json!({ "message": "", "room": "", "level": "info", "source": "workflow" })
            }
            NodeKind::DeliverAlert => json!({ "id": "" }),
            NodeKind::CameraSnapshot => json!({ "camera": "first" }),
            NodeKind::DescribeImage => json!({}),
            NodeKind::NotifyBeacon => json!({ "message": "", "target": "" }),
            NodeKind::Format => json!({ "template": "{p1} — {p2}", "p1": "", "p2": "", "p3": "" }),
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
    pub x: i32,
    pub y: i32,
    /// Static settings + literal fallbacks for unconnected input ports.
    #[serde(default)]
    pub config: Value,
}

impl GraphNode {
    pub fn center(&self) -> (i32, i32) {
        (self.x + NODE_W / 2, self.y + NODE_H / 2)
    }

    pub fn rect(&self) -> (i32, i32, i32, i32) {
        (self.x, self.y, NODE_W, NODE_H)
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
    #[serde(default = "one")]
    pub next_id: u32,
    #[serde(default)]
    pub nodes: Vec<GraphNode>,
    #[serde(default)]
    pub edges: Vec<Edge>,
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
            next_id: 1,
            nodes: Vec::new(),
            edges: Vec::new(),
        }
    }

    pub fn node(&self, id: u32) -> Option<&GraphNode> {
        self.nodes.iter().find(|n| n.id == id)
    }

    pub fn node_mut(&mut self, id: u32) -> Option<&mut GraphNode> {
        self.nodes.iter_mut().find(|n| n.id == id)
    }

    /// Add a node centred at (x, y); returns its new id.
    pub fn add_node(&mut self, kind: NodeKind, x: i32, y: i32) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        self.nodes.push(GraphNode {
            id,
            kind,
            x,
            y,
            config: kind.default_config(),
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
        // Distinct node-level adjacency + in-degrees.
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

/// The collection of named graphs persisted to `workflows.json`.
pub struct Workflows {
    pub graphs: Vec<Graph>,
    pub next_graph_id: u32,
    pub selected: usize,
}

impl Workflows {
    /// Load from disk (the real app path).
    pub fn new() -> Self {
        let graphs = load_workflows();
        Self::from_graphs(graphs)
    }

    /// In-memory collection (tests / never touches disk).
    pub fn in_memory() -> Self {
        Self::from_graphs(Vec::new())
    }

    fn from_graphs(graphs: Vec<Graph>) -> Self {
        let next_graph_id = graphs.iter().map(|g| g.id).max().map_or(1, |m| m + 1);
        Workflows {
            graphs,
            next_graph_id,
            selected: 0,
        }
    }

    pub fn create_graph(&mut self, name: &str) -> u32 {
        let id = self.next_graph_id;
        self.next_graph_id += 1;
        self.graphs.push(Graph::new(id, name));
        self.selected = self.graphs.len() - 1;
        id
    }

    pub fn remove_graph(&mut self, idx: usize) {
        if idx < self.graphs.len() {
            self.graphs.remove(idx);
            if self.selected >= self.graphs.len() {
                self.selected = self.graphs.len().saturating_sub(1);
            }
        }
    }

    pub fn selected_graph(&self) -> Option<&Graph> {
        self.graphs.get(self.selected)
    }

    pub fn selected_graph_mut(&mut self) -> Option<&mut Graph> {
        self.graphs.get_mut(self.selected)
    }

    pub fn save(&self) {
        save_workflows(&self.graphs);
    }
}

impl Default for Workflows {
    fn default() -> Self {
        Self::in_memory()
    }
}

fn workflows_path() -> PathBuf {
    PathBuf::from("workflows.json")
}

/// On-disk shape: one object holding the named-graph array.
#[derive(Default, Serialize, Deserialize)]
struct WorkflowsFile {
    #[serde(default)]
    graphs: Vec<Graph>,
}

pub fn load_workflows() -> Vec<Graph> {
    std::fs::read_to_string(workflows_path())
        .ok()
        .and_then(|s| serde_json::from_str::<WorkflowsFile>(&s).ok())
        .map(|f| f.graphs)
        .unwrap_or_default()
}

pub fn save_workflows(graphs: &[Graph]) {
    if cfg!(test) {
        return; // never clobber the real workflows.json from unit tests
    }
    let file = WorkflowsFile {
        graphs: graphs.to_vec(),
    };
    if let Ok(json) = serde_json::to_string_pretty(&file) {
        let _ = std::fs::write(workflows_path(), json);
    }
}

// ---------------------------------------------------------------------------
// Editor state (the WORKFLOWS screen) — mirrors `map::MapState`.
// ---------------------------------------------------------------------------

/// Canvas **world** size in cells — the coordinate space nodes can occupy. The
/// viewport (see [`WorkflowsState::viewport`]) shows a window into it. Bigger
/// than the floor-plan world since node boxes are wide.
pub const WF_GRID_W: i32 = 96;
pub const WF_GRID_H: i32 = 54;

/// Discrete zoom levels, as visible widths in cells (height derived from the
/// aspect). Mirrors `map`'s `ZOOM_SPANS`.
pub const WF_ZOOM_SPANS: [i32; 5] = [32, 48, 64, 80, 96];
pub const WF_DEFAULT_ZOOM: usize = 1;

/// The aspect the viewport holds while zooming.
const WF_VIEW_W: i32 = 48;
const WF_VIEW_H: i32 = 27;

/// Which sub-mode the editor is in. Arrow keys are reused per-mode. Mirrors
/// `map::MapMode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WfMode {
    Browse,
    /// Picking a `NodeKind` from the palette to drop on the canvas.
    Palette,
    /// Repositioning the selected node with the arrows.
    Move,
    /// Wiring: a source output port is chosen, then a target node + input port.
    Link,
    /// Per-node config form (and graph trigger/concurrency when no node).
    Config,
    /// Panning / zooming the viewport.
    Pan,
    /// The graph switcher: list / create / rename / delete / select-active.
    Graphs,
}

/// Which kind of field the Config form row currently sits on. The form mixes the
/// node's input-port literals + a few static keys with the graph-level trigger /
/// concurrency selectors, so the cursor walks a flat list of typed rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WfFieldKind {
    /// A free-text config key (typed into `config[key]`).
    Text,
    /// The alert `level` selector (Debug…Error), adjusted with ←/→.
    Level,
    /// The graph `trigger` selector, adjusted with ←/→ (and numeric typing).
    Trigger,
    /// The graph `concurrency` selector, adjusted with ←/→ (and numeric typing).
    Concurrency,
}

/// One editable row in the Config form.
#[derive(Debug, Clone)]
pub struct WfField {
    pub label: &'static str,
    pub key: &'static str,
    pub kind: WfFieldKind,
}

/// Transient editor state. The graph data is owned here (single-owner — runs
/// happen on cloned snapshots in background threads, never mutating this).
pub struct WorkflowsState {
    pub workflows: Workflows,
    /// Index into the selected graph's `nodes`.
    pub selected: usize,
    pub mode: WfMode,
    /// One-line feedback shown in amber, like `MapState.status`.
    pub status: String,
    /// Set by `R` (Browse) to ask the App to run the selected graph now; the App
    /// drains it in `handle_key` and clears it. Carries the graph id.
    pub run_request: Option<u32>,

    // -- Palette scratch ----------------------------------------------------
    /// Highlighted row in `NodeKind::ALL`.
    pub palette_cursor: usize,

    // -- Link scratch -------------------------------------------------------
    /// `(source node id, output-port index)` once the source port is locked in.
    pub link_from: Option<(u32, usize)>,
    /// Output-port index being cycled on the source node (before lock-in).
    pub link_out: usize,
    /// Input-port index being cycled on the target node.
    pub link_in: usize,

    // -- Config scratch -----------------------------------------------------
    /// Highlighted row in the Config form.
    pub config_cursor: usize,

    // -- Graphs switcher scratch -------------------------------------------
    /// Highlighted row in the graph list.
    pub graphs_cursor: usize,
    /// Whether the Graphs switcher is in name-entry (create/rename) mode.
    pub graphs_editing: bool,
    /// The text buffer for create/rename.
    pub graphs_buffer: String,
    /// Whether `graphs_buffer` renames the selected graph (vs. creates a new one).
    pub graphs_rename: bool,

    // -- Viewport (mirrors map) --------------------------------------------
    pub view_x: i32,
    pub view_y: i32,
    pub zoom: usize,
}

impl WorkflowsState {
    /// Load from disk (the real app path).
    pub fn new() -> Self {
        Self::from_workflows(Workflows::new())
    }

    /// In-memory state (tests / never touches disk).
    pub fn in_memory() -> Self {
        Self::from_workflows(Workflows::in_memory())
    }

    fn from_workflows(workflows: Workflows) -> Self {
        WorkflowsState {
            workflows,
            selected: 0,
            mode: WfMode::Browse,
            status: String::new(),
            run_request: None,
            palette_cursor: 0,
            link_from: None,
            link_out: 0,
            link_in: 0,
            config_cursor: 0,
            graphs_cursor: 0,
            graphs_editing: false,
            graphs_buffer: String::new(),
            graphs_rename: false,
            view_x: 0,
            view_y: 0,
            zoom: WF_DEFAULT_ZOOM,
        }
    }

    /// The selected graph, if any.
    pub fn graph(&self) -> Option<&Graph> {
        self.workflows.selected_graph()
    }

    pub fn graph_mut(&mut self) -> Option<&mut Graph> {
        self.workflows.selected_graph_mut()
    }

    /// The node the Browse cursor points at, in the selected graph.
    pub fn selected_node(&self) -> Option<&GraphNode> {
        self.graph().and_then(|g| g.nodes.get(self.selected))
    }

    /// The visible-window span `(width, height)` in cells for a zoom index.
    fn span(zoom: usize) -> (i32, i32) {
        let vw = WF_ZOOM_SPANS[zoom.min(WF_ZOOM_SPANS.len() - 1)].min(WF_GRID_W);
        let vh = (vw * WF_VIEW_H / WF_VIEW_W).clamp(1, WF_GRID_H);
        (vw, vh)
    }

    /// The current viewport `(x, y, w, h)` in world cells, clamped into the world.
    pub fn viewport(&self) -> (i32, i32, i32, i32) {
        let (vw, vh) = Self::span(self.zoom);
        let vx = self.view_x.clamp(0, (WF_GRID_W - vw).max(0));
        let vy = self.view_y.clamp(0, (WF_GRID_H - vh).max(0));
        (vx, vy, vw, vh)
    }

    fn clamp_view(&mut self) {
        let (vx, vy, _, _) = self.viewport();
        self.view_x = vx;
        self.view_y = vy;
    }

    /// Set the zoom level, keeping the viewport centre fixed, then re-clamp.
    fn set_zoom(&mut self, zoom: usize) {
        let (vx, vy, vw, vh) = self.viewport();
        let (cx, cy) = (vx + vw / 2, vy + vh / 2);
        self.zoom = zoom.min(WF_ZOOM_SPANS.len() - 1);
        let (nw, nh) = Self::span(self.zoom);
        self.view_x = cx - nw / 2;
        self.view_y = cy - nh / 2;
        self.clamp_view();
    }

    pub fn zoom_in(&mut self) {
        self.set_zoom(self.zoom.saturating_sub(1));
    }

    pub fn zoom_out(&mut self) {
        self.set_zoom(self.zoom + 1); // set_zoom caps at the last level
    }

    /// Pan the viewport by `(dx, dy)` steps, clamped.
    pub fn pan(&mut self, dx: i32, dy: i32) {
        const STEP: i32 = 3;
        self.view_x += dx * STEP;
        self.view_y += dy * STEP;
        self.clamp_view();
    }

    /// Auto-follow: if the selected node isn't fully inside the viewport,
    /// recentre the window on it. Called after selection and Move edits.
    pub fn ensure_visible(&mut self) {
        let Some((nx, ny)) = self.selected_node().map(|n| (n.x, n.y)) else { return };
        let (vx, vy, vw, vh) = self.viewport();
        let outside = nx < vx || ny < vy || nx + NODE_W > vx + vw || ny + NODE_H > vy + vh;
        if outside {
            self.view_x = (nx + NODE_W / 2) - vw / 2;
            self.view_y = (ny + NODE_H / 2) - vh / 2;
            self.clamp_view();
        }
    }

    /// Frame all nodes in the selected graph: the smallest zoom whose window
    /// contains their bounding box, centred on it. Resets to the default view when
    /// the graph is empty. (Mirrors `MapState::fit`.)
    pub fn fit(&mut self) {
        let bounds = self.graph().map(|g| {
            g.nodes.iter().fold(None, |acc: Option<(i32, i32, i32, i32)>, n| {
                let (nx0, ny0, nx1, ny1) = (n.x, n.y, n.x + NODE_W, n.y + NODE_H);
                Some(match acc {
                    None => (nx0, ny0, nx1, ny1),
                    Some((minx, miny, maxx, maxy)) => {
                        (minx.min(nx0), miny.min(ny0), maxx.max(nx1), maxy.max(ny1))
                    }
                })
            })
        });
        let Some(Some((minx, miny, maxx, maxy))) = bounds else {
            self.zoom = WF_DEFAULT_ZOOM;
            self.view_x = 0;
            self.view_y = 0;
            return;
        };
        let (bw, bh) = (maxx - minx, maxy - miny);
        let zoom = (0..WF_ZOOM_SPANS.len())
            .find(|&z| {
                let (w, h) = Self::span(z);
                w >= bw && h >= bh
            })
            .unwrap_or(WF_ZOOM_SPANS.len() - 1);
        self.zoom = zoom;
        let (w, h) = Self::span(zoom);
        self.view_x = (minx + bw / 2) - w / 2;
        self.view_y = (miny + bh / 2) - h / 2;
        self.clamp_view();
    }
}

impl Default for WorkflowsState {
    fn default() -> Self {
        Self::in_memory()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

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
        // create_alert default config carries the static source="workflow".
        assert_eq!(g.node(a).unwrap().config["source"], "workflow");
    }

    #[test]
    fn remove_node_cascades_edges() {
        let (mut g, a, b) = chain();
        assert_eq!(g.edges.len(), 1);
        g.remove_node(a);
        assert!(g.node(a).is_none());
        assert_eq!(g.edges.len(), 0, "edges touching a removed node are dropped");
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
    fn add_edge_rejects_self_loop() {
        let mut g = Graph::new(1, "g");
        let f = g.add_node(NodeKind::Format, 0, 0);
        assert!(g.add_edge(f, "text", f, "p1").is_err());
    }

    #[test]
    fn add_edge_rejects_type_mismatch() {
        let mut g = Graph::new(1, "g");
        let snap = g.add_node(NodeKind::CameraSnapshot, 0, 0);
        let fmt = g.add_node(NodeKind::Format, 0, 0);
        // image (Image) -> p1 (Text) must be rejected.
        let err = g.add_edge(snap, "image", fmt, "p1").unwrap_err();
        assert!(err.contains("type mismatch"), "got: {err}");
        assert_eq!(g.edges.len(), 0);
    }

    #[test]
    fn add_edge_rejects_double_fed_input() {
        let mut g = Graph::new(1, "g");
        let a = g.add_node(NodeKind::GetLastAlert, 0, 0);
        let f = g.add_node(NodeKind::Format, 0, 0);
        let b = g.add_node(NodeKind::CreateAlert, 0, 0);
        g.add_edge(a, "message", b, "message").unwrap();
        // a second wire into create_alert.message is rejected.
        let err = g.add_edge(f, "text", b, "message").unwrap_err();
        assert!(err.contains("already wired"), "got: {err}");
    }

    #[test]
    fn add_edge_rejects_cycle() {
        let mut g = Graph::new(1, "g");
        let a = g.add_node(NodeKind::Format, 0, 0);
        let b = g.add_node(NodeKind::Format, 0, 0);
        g.add_edge(a, "text", b, "p1").unwrap();
        let err = g.add_edge(b, "text", a, "p1").unwrap_err();
        assert!(err.contains("cycle"), "got: {err}");
    }

    #[test]
    fn add_edge_rejects_unknown_port() {
        let mut g = Graph::new(1, "g");
        let a = g.add_node(NodeKind::GetLastAlert, 0, 0);
        let b = g.add_node(NodeKind::CreateAlert, 0, 0);
        assert!(g.add_edge(a, "nope", b, "message").is_err());
        assert!(g.add_edge(a, "message", b, "nope").is_err());
    }

    #[test]
    fn fan_in_two_inputs_resolve_and_order() {
        // get_last_alert.message -> notify_beacon.message ;
        // camera_snapshot.image  -> notify_beacon.image
        let mut g = Graph::new(1, "g");
        let alert = g.add_node(NodeKind::GetLastAlert, 0, 0);
        let snap = g.add_node(NodeKind::CameraSnapshot, 0, 0);
        let notify = g.add_node(NodeKind::NotifyBeacon, 0, 0);
        g.add_edge(alert, "message", notify, "message").unwrap();
        g.add_edge(snap, "image", notify, "image").unwrap();
        let order = g.topo_order().unwrap();
        // notify must come after both upstream sources.
        let pos = |id| order.iter().position(|&x| x == id).unwrap();
        assert!(pos(notify) > pos(alert) && pos(notify) > pos(snap));
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
    fn workflows_collection_create_and_select() {
        let mut w = Workflows::in_memory();
        let id1 = w.create_graph("alpha");
        let id2 = w.create_graph("beta");
        assert_ne!(id1, id2);
        assert_eq!(w.graphs.len(), 2);
        assert_eq!(w.selected, 1);
        assert_eq!(w.selected_graph().unwrap().name, "beta");
        w.remove_graph(1);
        assert_eq!(w.graphs.len(), 1);
        assert_eq!(w.selected_graph().unwrap().name, "alpha");
    }

    #[test]
    fn all_node_kinds_have_consistent_ports() {
        for k in NodeKind::ALL {
            // no duplicate port names; required only on inputs.
            let mut names = HashSet::new();
            for port in k.input_ports().iter().chain(k.output_ports()) {
                assert!(names.insert((port.name, port_dir(k, port))), "dup port on {k:?}");
            }
        }
    }

    fn port_dir(k: NodeKind, port: &Port) -> bool {
        k.input_ports().iter().any(|q| q.name == port.name && q.ty == port.ty)
    }
}
