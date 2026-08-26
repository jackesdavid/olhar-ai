//! The graph document: what is stored, and how it is edited.
//!
//! Topology — nodes, edges, the viewport — belongs to the engine and is shared. What lives here is
//! `SentinelMeta`, the part that is this product's: the trigger, the instance scope, the declared
//! variables, whether debug is on.
//!
//! [`GraphExt`] is the editing surface, and `wire` is the interesting one: it refuses a connection
//! whose port types disagree, which is why a saved graph cannot contain a wire the engine would
//! choke on later.

use super::*;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

// ---------------------------------------------------------------------------
// Graph
// ---------------------------------------------------------------------------

/// One named automation: the engine's document, plus this product's policy.
///
/// A type alias rather than a struct of our own. Everything the engine understands about a graph
/// — id, name, nodes, edges, the next id to hand out — is the engine's; everything this product
/// decides about one lives in [`SentinelMeta`](crate::workflows::host::SentinelMeta), spliced
/// into the same JSON object with `#[serde(flatten)]`.
///
/// What this replaces is a parallel struct with its own wiring rules, its own topological sort
/// and its own idea of when an edge is legal. Two implementations of "may this wire exist" is
/// how an editor starts drawing a link the scheduler then refuses.

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

pub type Graph = ggraph_core::Graph<crate::workflows::host::SentinelMeta>;

/// What this product's graphs can do beyond what any graph can.
///
/// `wire` rather than `add_edge`: the engine's inherent `add_edge` takes a registry, and an
/// inherent method wins name resolution over a trait one. The different name is honest —
/// this one already knows which registry to ask.
pub trait GraphExt {
    /// Add a node **with its kind's default configuration**.
    ///
    /// The engine's `add_node` leaves the config empty, and it has to: it does not know a
    /// registry, so it cannot know what a kind's defaults are. Seeding them is this product's
    /// job, and a node arriving on the canvas without them is one whose inspector opens blank.
    fn add(&mut self, kind: NodeKind, x: i32, y: i32) -> u32;
    fn wire(&mut self, from: u32, from_port: &str, to: u32, to_port: &str) -> Result<(), String>;
    fn resolve_camera_selector(&self, node: &GraphNode) -> Option<String>;
    fn resolve_region(&self, node: &GraphNode) -> Option<(String, Vec<(f32, f32)>)>;
    fn roots(&self) -> Vec<u32>;
    fn remove_edge(&mut self, idx: usize);
    /// Topological node order (stable: follows node insertion order on ties).
    /// `Err` if the graph contains a cycle.
    fn topo_order(&self) -> Result<Vec<u32>, String>;
    fn back_edges(&self) -> HashSet<(u32, u32)>;
}

impl GraphExt for Graph {
    fn add(&mut self, kind: NodeKind, x: i32, y: i32) -> u32 {
        let id = self.add_node(kind, x, y);
        if let Some(n) = self.node_mut(id) {
            n.config = kind.default_config();
        }
        id
    }

    fn wire(&mut self, from: u32, from_port: &str, to: u32, to_port: &str) -> Result<(), String> {
        self.add_edge(
            crate::workflows::registry::registry(),
            from,
            from_port,
            to,
            to_port,
        )
        .map_err(|e| e.to_string())
    }

    /// What `node`'s `camera` input currently resolves to (an id/ip/name selector, NOT yet
    /// resolved to a device — callers do that against their own device list): the wired
    /// `GetCameraByName` node's own `config.camera`, or (no wire) `node`'s OWN `config.camera`
    /// literal — the same wire-overrides-config pattern as `ObjectDetection`'s `class` input.
    /// `None` if neither is set, meaning "unscoped" to callers like `on_event_node`.
    fn resolve_camera_selector(&self, node: &GraphNode) -> Option<String> {
        let cfg_str = |n: &GraphNode| {
            n.config
                .get("camera")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
        };
        self.wired_source(node.id, "camera")
            .and_then(|id| self.node(id))
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
    fn resolve_region(&self, node: &GraphNode) -> Option<(String, Vec<(f32, f32)>)> {
        let region_node = self
            .wired_source(node.id, "region")
            .and_then(|id| self.node(id))
            .filter(|n| n.kind == NodeKind::Region)?;
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

    /// Nodes with no incoming edge (sources / fully-literal nodes).
    fn roots(&self) -> Vec<u32> {
        self.nodes
            .iter()
            .map(|n| n.id)
            .filter(|id| !self.edges.iter().any(|e| e.to == *id))
            .collect()
    }

    fn remove_edge(&mut self, idx: usize) {
        if idx < self.edges.len() {
            self.edges.remove(idx);
        }
    }

    /// Topological node order (stable: follows node insertion order on ties).
    /// `Err` if the graph contains a cycle.
    fn topo_order(&self) -> Result<Vec<u32>, String> {
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
    fn back_edges(&self) -> HashSet<(u32, u32)> {
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
