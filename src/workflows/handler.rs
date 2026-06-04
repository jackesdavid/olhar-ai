//! Workflows editing logic: node selection, drop (palette), move, typed linking,
//! per-node + graph config, and the named-graph switcher. Pure state mutation —
//! background runs happen on cloned graph snapshots in `App` (Phase 6), so this
//! module never spawns threads or touches the scan/alert stores.
use serde_json::{json, Value};

use super::domain::{
    save_workflows, AlertLevel, ConcurrencyPolicy, NodeKind, Trigger, WfField, WfFieldKind, WfMode,
    WorkflowsState, NODE_H, NODE_W, WF_GRID_H, WF_GRID_W,
};

impl WorkflowsState {
    /// Persist the whole graph collection (no-op in tests, like `map::save_map`).
    fn save(&self) {
        save_workflows(&self.workflows.graphs);
    }

    // ---- node selection (mirrors MapState::{next,prev,clamp}) --------------
    pub fn next(&mut self) {
        let n = self.graph().map_or(0, |g| g.nodes.len());
        if n > 0 {
            self.selected = (self.selected + 1) % n;
            self.ensure_visible();
        }
    }

    pub fn prev(&mut self) {
        let n = self.graph().map_or(0, |g| g.nodes.len());
        if n > 0 {
            self.selected = (self.selected + n - 1) % n;
            self.ensure_visible();
        }
    }

    fn clamp(&mut self) {
        let n = self.graph().map_or(0, |g| g.nodes.len());
        self.selected = if n == 0 { 0 } else { self.selected.min(n - 1) };
    }

    /// Id of the node the Browse cursor points at, in the selected graph.
    fn selected_node_id(&self) -> Option<u32> {
        self.selected_node().map(|n| n.id)
    }

    // ---- palette (drop a node) --------------------------------------------
    pub fn enter_palette(&mut self) {
        if self.graph().is_none() {
            self.status = "Create a graph first ([G] → [N]).".into();
            return;
        }
        self.palette_cursor = 0;
        self.mode = WfMode::Palette;
        self.status = "PALETTE \u{2014} Up/Down pick, Enter drop, Esc cancel".into();
    }

    pub fn palette_next(&mut self) {
        self.palette_cursor = (self.palette_cursor + 1) % NodeKind::ALL.len();
    }

    pub fn palette_prev(&mut self) {
        let n = NodeKind::ALL.len();
        self.palette_cursor = (self.palette_cursor + n - 1) % n;
    }

    /// Drop the highlighted kind near the viewport centre and select it.
    pub fn drop_palette_node(&mut self) {
        let kind = NodeKind::ALL[self.palette_cursor.min(NodeKind::ALL.len() - 1)];
        let (vx, vy, vw, vh) = self.viewport();
        let x = (vx + vw / 2 - NODE_W / 2).clamp(0, WF_GRID_W - NODE_W);
        let y = (vy + vh / 2 - NODE_H / 2).clamp(0, WF_GRID_H - NODE_H);
        if let Some(g) = self.graph_mut() {
            g.add_node(kind, x, y);
            self.selected = g.nodes.len() - 1;
        }
        self.mode = WfMode::Browse;
        self.status = format!("Added {}.", kind.label());
        self.save();
    }

    // ---- delete -----------------------------------------------------------
    pub fn delete_node(&mut self) {
        let Some(id) = self.selected_node_id() else { return };
        let label = self.selected_node().map(|n| n.kind.label()).unwrap_or("node");
        let status = format!("Removed {label}.");
        if let Some(g) = self.graph_mut() {
            g.remove_node(id);
        }
        self.clamp();
        self.status = status;
        self.save();
    }

    // ---- move -------------------------------------------------------------
    pub fn enter_move(&mut self) {
        if self.selected_node().is_some() {
            self.mode = WfMode::Move;
            self.status = "MOVE \u{2014} arrows reposition, Enter/Esc done".into();
        }
    }

    /// Translate the selected node by (dx,dy) cells, clamped into the world.
    pub fn nudge(&mut self, dx: i32, dy: i32) {
        let idx = self.selected;
        if let Some(g) = self.graph_mut() {
            if let Some(n) = g.nodes.get_mut(idx) {
                n.x = (n.x + dx).clamp(0, WF_GRID_W - NODE_W);
                n.y = (n.y + dy).clamp(0, WF_GRID_H - NODE_H);
            }
        }
        self.ensure_visible();
    }

    pub fn commit_move(&mut self) {
        self.mode = WfMode::Browse;
        self.status.clear();
        self.save();
    }

    // ---- view navigation (Pan / zoom) -------------------------------------
    pub fn enter_pan(&mut self) {
        self.mode = WfMode::Pan;
        self.status = "PAN \u{2014} arrows move view, +/- zoom, Enter/Esc done".into();
    }

    pub fn exit_pan(&mut self) {
        self.mode = WfMode::Browse;
        self.status.clear();
    }

    // ---- linking (typed wiring) -------------------------------------------
    /// Begin a link from the selected node: start cycling its output ports.
    pub fn enter_link(&mut self) {
        let Some(node) = self.selected_node() else { return };
        if node.kind.output_ports().is_empty() {
            self.status = format!("{} has no outputs.", node.kind.label());
            return;
        }
        self.link_from = None;
        self.link_out = 0;
        self.link_in = 0;
        self.mode = WfMode::Link;
        self.status = "LINK \u{2014} pick output (\u{2190}/\u{2192}), Enter, then target".into();
    }

    /// Cycle the source output port (before lock-in) or the target node + input
    /// port (after lock-in), with `delta` = -1 / +1.
    pub fn link_cycle(&mut self, delta: i32) {
        match self.link_from {
            None => {
                if let Some(node) = self.selected_node() {
                    let n = node.kind.output_ports().len().max(1);
                    self.link_out = wrap(self.link_out, delta, n);
                }
            }
            Some((from_id, _)) => {
                // Cycle the target node selection (skip the source), then reset port.
                let n = self.graph().map_or(0, |g| g.nodes.len());
                if n > 1 {
                    loop {
                        self.selected = wrap(self.selected, delta, n);
                        if self.selected_node_id() != Some(from_id) {
                            break;
                        }
                    }
                    self.link_in = 0;
                    self.ensure_visible();
                }
            }
        }
    }

    /// Cycle the target input port (after a target node is being chosen).
    pub fn link_cycle_input(&mut self, delta: i32) {
        if self.link_from.is_some() {
            if let Some(node) = self.selected_node() {
                let n = node.kind.input_ports().len().max(1);
                self.link_in = wrap(self.link_in, delta, n);
            }
        }
    }

    /// Enter on Link: lock the source output port first, then (on the second
    /// Enter) commit the typed edge, surfacing any validation error.
    pub fn link_enter(&mut self) {
        match self.link_from {
            None => {
                let Some((id, kind)) = self.selected_node().map(|n| (n.id, n.kind)) else { return };
                self.link_from = Some((id, self.link_out));
                let port = kind.output_ports().get(self.link_out).map(|p| p.name).unwrap_or("");
                self.status =
                    format!("FROM {}.{port} \u{2014} pick target (\u{2191}/\u{2193}), input (\u{2190}/\u{2192}), Enter", kind.label());
            }
            Some((from_id, out_idx)) => {
                let Some(to_node) = self.selected_node() else { return };
                let to_id = to_node.id;
                let (Some(from_kind), Some(to_kind)) = (
                    self.graph().and_then(|g| g.node(from_id)).map(|n| n.kind),
                    Some(to_node.kind),
                ) else {
                    return;
                };
                let from_port = match from_kind.output_ports().get(out_idx) {
                    Some(p) => p.name,
                    None => return,
                };
                let to_port = match to_kind.input_ports().get(self.link_in) {
                    Some(p) => p.name,
                    None => {
                        self.status = format!("{} has no inputs.", to_kind.label());
                        return;
                    }
                };
                let res = self
                    .graph_mut()
                    .map(|g| g.add_edge(from_id, from_port, to_id, to_port))
                    .unwrap_or(Ok(()));
                match res {
                    Ok(()) => {
                        self.status = format!("Linked {from_port} \u{2192} {to_port}.");
                        self.link_from = None;
                        self.mode = WfMode::Browse;
                        self.save();
                    }
                    Err(e) => self.status = format!("\u{2717} {e}"),
                }
            }
        }
    }

    pub fn cancel_link(&mut self) {
        self.link_from = None;
        self.mode = WfMode::Browse;
        self.status.clear();
    }

    // ---- enabled / run ----------------------------------------------------
    /// Toggle the selected graph's `enabled` flag and persist.
    pub fn toggle_enabled(&mut self) {
        let on = match self.graph_mut() {
            Some(g) => {
                g.enabled = !g.enabled;
                g.enabled
            }
            None => return,
        };
        let name = self.graph().map(|g| g.name.clone()).unwrap_or_default();
        self.status = format!("'{name}' {}", if on { "enabled" } else { "disabled" });
        self.save();
    }

    /// Request a manual run of the selected graph. The App drains `run_request`.
    pub fn request_run(&mut self) {
        match self.graph().map(|g| (g.id, g.name.clone(), g.nodes.is_empty())) {
            Some((_, _, true)) => self.status = "Graph is empty \u{2014} nothing to run.".into(),
            Some((id, name, false)) => {
                self.run_request = Some(id);
                self.status = format!("Running '{name}'\u{2026}");
            }
            None => self.status = "No graph selected.".into(),
        }
    }

    // ---- config form ------------------------------------------------------
    /// The flat list of editable rows for the current Config target: the selected
    /// node's literal fields, then the graph-level trigger + concurrency.
    pub fn config_fields(&self) -> Vec<WfField> {
        let mut fields = Vec::new();
        if let Some(node) = self.selected_node() {
            // Static text/level keys carried in the node's default config.
            match node.kind {
                NodeKind::CreateAlert => {
                    fields.push(WfField { label: "MESSAGE", key: "message", kind: WfFieldKind::Text });
                    fields.push(WfField { label: "ROOM", key: "room", kind: WfFieldKind::Text });
                    fields.push(WfField { label: "LEVEL", key: "level", kind: WfFieldKind::Level });
                    fields.push(WfField { label: "SOURCE", key: "source", kind: WfFieldKind::Text });
                }
                NodeKind::DeliverAlert => {
                    fields.push(WfField { label: "ALERT ID", key: "id", kind: WfFieldKind::Text });
                }
                NodeKind::CameraSnapshot => {
                    fields.push(WfField { label: "CAMERA", key: "camera", kind: WfFieldKind::Text });
                }
                NodeKind::DescribeImage => {
                    fields.push(WfField { label: "LABEL", key: "label", kind: WfFieldKind::Text });
                }
                NodeKind::NotifyBeacon => {
                    fields.push(WfField { label: "MESSAGE", key: "message", kind: WfFieldKind::Text });
                    fields.push(WfField { label: "TARGET", key: "target", kind: WfFieldKind::Text });
                }
                NodeKind::Format => {
                    fields.push(WfField { label: "TEMPLATE", key: "template", kind: WfFieldKind::Text });
                    fields.push(WfField { label: "P1", key: "p1", kind: WfFieldKind::Text });
                    fields.push(WfField { label: "P2", key: "p2", kind: WfFieldKind::Text });
                    fields.push(WfField { label: "P3", key: "p3", kind: WfFieldKind::Text });
                }
                NodeKind::GetLastAlert => {}
            }
        }
        // Graph-level selectors always sit at the bottom of the form.
        fields.push(WfField { label: "TRIGGER", key: "__trigger", kind: WfFieldKind::Trigger });
        fields.push(WfField { label: "CONCURRENCY", key: "__concurrency", kind: WfFieldKind::Concurrency });
        fields
    }

    pub fn enter_config(&mut self) {
        if self.graph().is_none() {
            self.status = "Create a graph first ([G] → [N]).".into();
            return;
        }
        self.config_cursor = 0;
        self.mode = WfMode::Config;
        self.status = "CONFIG \u{2014} Up/Down field, type/\u{2190}\u{2192} edit, Enter save".into();
    }

    pub fn config_next(&mut self) {
        let n = self.config_fields().len().max(1);
        self.config_cursor = (self.config_cursor + 1) % n;
    }

    pub fn config_prev(&mut self) {
        let n = self.config_fields().len().max(1);
        self.config_cursor = (self.config_cursor + n - 1) % n;
    }

    fn current_field(&self) -> Option<WfField> {
        self.config_fields().into_iter().nth(self.config_cursor)
    }

    /// Type a character into the focused text field (node config literal).
    pub fn config_type(&mut self, c: char) {
        let Some(field) = self.current_field() else { return };
        if field.kind != WfFieldKind::Text {
            return;
        }
        let idx = self.selected;
        if let Some(g) = self.graph_mut() {
            if let Some(n) = g.nodes.get_mut(idx) {
                let entry = n.config.get(field.key).and_then(Value::as_str).unwrap_or("").to_string();
                n.config[field.key] = json!(format!("{entry}{c}"));
            }
        }
    }

    pub fn config_backspace(&mut self) {
        let Some(field) = self.current_field() else { return };
        if field.kind != WfFieldKind::Text {
            return;
        }
        let idx = self.selected;
        if let Some(g) = self.graph_mut() {
            if let Some(n) = g.nodes.get_mut(idx) {
                let mut entry = n.config.get(field.key).and_then(Value::as_str).unwrap_or("").to_string();
                entry.pop();
                n.config[field.key] = json!(entry);
            }
        }
    }

    /// Adjust the focused selector field (Level / Trigger / Concurrency) by a
    /// -1 / +1 nudge.
    pub fn config_adjust(&mut self, delta: i8) {
        let Some(field) = self.current_field() else { return };
        let idx = self.selected;
        match field.kind {
            WfFieldKind::Text => {}
            WfFieldKind::Level => {
                if let Some(g) = self.graph_mut() {
                    if let Some(n) = g.nodes.get_mut(idx) {
                        let cur = n
                            .config
                            .get("level")
                            .and_then(Value::as_str)
                            .and_then(AlertLevel::parse)
                            .unwrap_or_default();
                        let next = cur.cycled(delta);
                        n.config["level"] = json!(next.label().to_ascii_lowercase());
                    }
                }
            }
            WfFieldKind::Trigger => {
                if let Some(g) = self.graph_mut() {
                    g.trigger = cycle_trigger(&g.trigger, delta);
                }
            }
            WfFieldKind::Concurrency => {
                if let Some(g) = self.graph_mut() {
                    g.concurrency = cycle_concurrency(&g.concurrency, delta);
                }
            }
        }
    }

    pub fn commit_config(&mut self) {
        self.mode = WfMode::Browse;
        self.status = "Saved.".into();
        self.save();
    }

    pub fn cancel_config(&mut self) {
        self.mode = WfMode::Browse;
        self.status.clear();
    }

    // ---- graph switcher ---------------------------------------------------
    pub fn enter_graphs(&mut self) {
        self.graphs_cursor = self.workflows.selected;
        self.graphs_editing = false;
        self.graphs_buffer.clear();
        self.graphs_rename = false;
        self.mode = WfMode::Graphs;
        self.status = "GRAPHS \u{2014} Up/Down, Enter select, N new, R rename, X delete, Esc back".into();
    }

    pub fn graphs_next(&mut self) {
        let n = self.workflows.graphs.len();
        if n > 0 {
            self.graphs_cursor = (self.graphs_cursor + 1) % n;
        }
    }

    pub fn graphs_prev(&mut self) {
        let n = self.workflows.graphs.len();
        if n > 0 {
            self.graphs_cursor = (self.graphs_cursor + n - 1) % n;
        }
    }

    /// Select the highlighted graph as active and return to Browse.
    pub fn graphs_select(&mut self) {
        if self.graphs_cursor < self.workflows.graphs.len() {
            self.workflows.selected = self.graphs_cursor;
            self.selected = 0;
            self.mode = WfMode::Browse;
            self.status.clear();
            self.fit(); // frame the newly-selected graph's nodes
        }
    }

    /// Begin entering a name for a brand-new graph.
    pub fn graphs_begin_create(&mut self) {
        self.graphs_editing = true;
        self.graphs_rename = false;
        self.graphs_buffer.clear();
        self.status = "NAME \u{2014} type, Enter create, Esc cancel".into();
    }

    /// Begin renaming the highlighted graph.
    pub fn graphs_begin_rename(&mut self) {
        let Some(g) = self.workflows.graphs.get(self.graphs_cursor) else { return };
        self.graphs_editing = true;
        self.graphs_rename = true;
        self.graphs_buffer = g.name.clone();
        self.status = "RENAME \u{2014} type, Enter save, Esc cancel".into();
    }

    pub fn graphs_type(&mut self, c: char) {
        if self.graphs_editing {
            self.graphs_buffer.push(c);
        }
    }

    pub fn graphs_backspace(&mut self) {
        if self.graphs_editing {
            self.graphs_buffer.pop();
        }
    }

    /// Commit a create / rename buffer (falls back to a default name if blank on
    /// create).
    pub fn graphs_commit_edit(&mut self) {
        let name = {
            let t = self.graphs_buffer.trim();
            if t.is_empty() {
                if self.graphs_rename {
                    self.graphs_editing = false;
                    return; // a blank rename is a no-op
                }
                format!("GRAPH {:02}", self.workflows.next_graph_id)
            } else {
                t.to_string()
            }
        };
        if self.graphs_rename {
            if let Some(g) = self.workflows.graphs.get_mut(self.graphs_cursor) {
                g.name = name.clone();
            }
            self.status = format!("Renamed to '{name}'.");
        } else {
            self.workflows.create_graph(&name);
            self.graphs_cursor = self.workflows.selected;
            self.selected = 0;
            self.status = format!("Created '{name}'.");
        }
        self.graphs_editing = false;
        self.graphs_buffer.clear();
        self.save();
    }

    pub fn graphs_cancel_edit(&mut self) {
        self.graphs_editing = false;
        self.graphs_buffer.clear();
        self.status.clear();
    }

    /// Delete the highlighted graph and re-clamp the cursors.
    pub fn graphs_delete(&mut self) {
        if self.graphs_cursor >= self.workflows.graphs.len() {
            return;
        }
        let name = self.workflows.graphs[self.graphs_cursor].name.clone();
        self.workflows.remove_graph(self.graphs_cursor);
        let n = self.workflows.graphs.len();
        self.graphs_cursor = if n == 0 { 0 } else { self.graphs_cursor.min(n - 1) };
        self.selected = 0;
        self.status = format!("Removed '{name}'.");
        self.save();
    }
}

/// Wrap `i` by `delta` (-1/+1) within `[0, n)`.
fn wrap(i: usize, delta: i32, n: usize) -> usize {
    if n == 0 {
        return 0;
    }
    let n_i = n as i32;
    (((i as i32 + delta) % n_i + n_i) % n_i) as usize
}

/// Cycle a `Trigger` through Manual → Cron → OnNewAlert (preserving any payload
/// across re-entry by using sensible defaults).
fn cycle_trigger(t: &Trigger, delta: i8) -> Trigger {
    const ORDER: usize = 3;
    let cur = match t {
        Trigger::Manual => 0,
        Trigger::Cron { .. } => 1,
        Trigger::OnNewAlert { .. } => 2,
    };
    let next = (((cur as i8 + delta) % ORDER as i8 + ORDER as i8) % ORDER as i8) as usize;
    match next {
        1 => {
            let every = match t {
                Trigger::Cron { every_secs } => *every_secs,
                _ => 60,
            };
            Trigger::Cron { every_secs: every }
        }
        2 => {
            let min = match t {
                Trigger::OnNewAlert { min_level } => *min_level,
                _ => AlertLevel::Warn,
            };
            Trigger::OnNewAlert { min_level: min }
        }
        _ => Trigger::Manual,
    }
}

/// Cycle a `ConcurrencyPolicy` through SkipIfRunning → QueueSingle → Concurrent.
fn cycle_concurrency(c: &ConcurrencyPolicy, delta: i8) -> ConcurrencyPolicy {
    const ORDER: usize = 3;
    let cur = match c {
        ConcurrencyPolicy::SkipIfRunning => 0,
        ConcurrencyPolicy::QueueSingle { .. } => 1,
        ConcurrencyPolicy::Concurrent { .. } => 2,
    };
    let next = (((cur as i8 + delta) % ORDER as i8 + ORDER as i8) % ORDER as i8) as usize;
    match next {
        1 => ConcurrencyPolicy::QueueSingle { max_queued: 1 },
        2 => ConcurrencyPolicy::Concurrent { max_concurrent: 2 },
        _ => ConcurrencyPolicy::SkipIfRunning,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workflows::domain::NodeKind;

    /// An in-memory editor seeded with one named graph (never touches disk).
    fn state() -> WorkflowsState {
        let mut s = WorkflowsState::in_memory();
        s.workflows.create_graph("test");
        s
    }

    #[test]
    fn drop_palette_node_adds_and_selects() {
        let mut s = state();
        s.enter_palette();
        s.palette_next(); // CreateAlert (index 1)
        s.drop_palette_node();
        assert_eq!(s.graph().unwrap().nodes.len(), 1);
        assert_eq!(s.graph().unwrap().nodes[0].kind, NodeKind::CreateAlert);
        assert_eq!(s.selected, 0);
        assert_eq!(s.mode, WfMode::Browse);
    }

    #[test]
    fn fit_frames_all_nodes() {
        use crate::workflows::domain::{NODE_H, NODE_W};
        let mut s = state();
        {
            let g = s.graph_mut().unwrap();
            g.add_node(NodeKind::GetLastAlert, 5, 5);
            g.add_node(NodeKind::NotifyBeacon, 50, 30);
        }
        s.fit();
        let (vx, vy, vw, vh) = s.viewport();
        let nodes: Vec<(i32, i32)> =
            s.graph().unwrap().nodes.iter().map(|n| (n.x, n.y)).collect();
        for (x, y) in nodes {
            assert!(
                x >= vx && y >= vy && x + NODE_W <= vx + vw && y + NODE_H <= vy + vh,
                "node ({x},{y}) not framed by viewport ({vx},{vy},{vw},{vh})"
            );
        }
    }

    #[test]
    fn link_two_nodes_creates_edge() {
        let mut s = state();
        // get_last_alert.message -> create_alert.message
        let (a, b) = {
            let g = s.graph_mut().unwrap();
            (g.add_node(NodeKind::GetLastAlert, 0, 0), g.add_node(NodeKind::CreateAlert, 30, 0))
        };
        // Select source, enter link, pick the "message" output port.
        s.selected = s.graph().unwrap().nodes.iter().position(|n| n.id == a).unwrap();
        s.enter_link();
        // GetLastAlert outputs: id,level,message,... — cycle to "message" (idx 2).
        s.link_out = 2;
        s.link_enter(); // lock the source port
        assert!(s.link_from.is_some());
        // Move the target cursor to b, choose its "message" input (idx 0).
        s.selected = s.graph().unwrap().nodes.iter().position(|n| n.id == b).unwrap();
        s.link_in = 0;
        s.link_enter(); // commit
        let g = s.graph().unwrap();
        assert_eq!(g.edges.len(), 1);
        assert_eq!(g.edges[0].from, a);
        assert_eq!(g.edges[0].to, b);
        assert_eq!(s.mode, WfMode::Browse);
    }

    #[test]
    fn bad_link_surfaces_error_and_keeps_no_edge() {
        let mut s = state();
        // camera_snapshot.image (Image) -> format.p1 (Text) is a type mismatch.
        let (snap, fmt) = {
            let g = s.graph_mut().unwrap();
            (g.add_node(NodeKind::CameraSnapshot, 0, 0), g.add_node(NodeKind::Format, 30, 0))
        };
        s.selected = s.graph().unwrap().nodes.iter().position(|n| n.id == snap).unwrap();
        s.enter_link();
        s.link_out = 0; // "image"
        s.link_enter();
        s.selected = s.graph().unwrap().nodes.iter().position(|n| n.id == fmt).unwrap();
        s.link_in = 0; // "p1"
        s.link_enter();
        assert!(s.status.contains("type mismatch"), "got: {}", s.status);
        assert_eq!(s.graph().unwrap().edges.len(), 0);
        assert_eq!(s.mode, WfMode::Link, "stays in link mode on failure");
    }

    #[test]
    fn toggle_enabled_flips_and_persists_flag() {
        let mut s = state();
        assert!(!s.graph().unwrap().enabled);
        s.toggle_enabled();
        assert!(s.graph().unwrap().enabled);
        s.toggle_enabled();
        assert!(!s.graph().unwrap().enabled);
    }

    #[test]
    fn delete_node_clamps_selection() {
        let mut s = state();
        {
            let g = s.graph_mut().unwrap();
            g.add_node(NodeKind::Format, 0, 0);
            g.add_node(NodeKind::Format, 20, 0);
            g.add_node(NodeKind::Format, 40, 0);
        }
        s.selected = 2;
        s.delete_node();
        assert_eq!(s.selected, 1);
        s.delete_node();
        s.delete_node();
        assert_eq!(s.selected, 0);
        assert!(s.graph().unwrap().nodes.is_empty());
    }

    #[test]
    fn config_text_edit_writes_into_node_config() {
        let mut s = state();
        let id = s.graph_mut().unwrap().add_node(NodeKind::Format, 0, 0);
        s.selected = s.graph().unwrap().nodes.iter().position(|n| n.id == id).unwrap();
        s.enter_config();
        // First field is TEMPLATE (text). Backspace the default then type.
        for _ in 0..40 {
            s.config_backspace();
        }
        for c in "hi".chars() {
            s.config_type(c);
        }
        assert_eq!(s.graph().unwrap().nodes[0].config["template"], "hi");
    }

    #[test]
    fn config_adjust_cycles_graph_trigger() {
        let mut s = state();
        s.graph_mut().unwrap().add_node(NodeKind::GetLastAlert, 0, 0);
        s.enter_config();
        // GetLastAlert has no literal fields, so row 0 is TRIGGER.
        assert!(matches!(s.graph().unwrap().trigger, Trigger::Manual));
        s.config_adjust(1);
        assert!(matches!(s.graph().unwrap().trigger, Trigger::Cron { .. }));
        s.config_adjust(1);
        assert!(matches!(s.graph().unwrap().trigger, Trigger::OnNewAlert { .. }));
        s.config_adjust(1);
        assert!(matches!(s.graph().unwrap().trigger, Trigger::Manual));
    }

    #[test]
    fn graphs_create_and_delete() {
        let mut s = state(); // already has "test"
        s.enter_graphs();
        s.graphs_begin_create();
        for c in "alpha".chars() {
            s.graphs_type(c);
        }
        s.graphs_commit_edit();
        assert_eq!(s.workflows.graphs.len(), 2);
        assert_eq!(s.graph().unwrap().name, "alpha");
        // Delete it; cursor + selection re-clamp without panic.
        s.graphs_cursor = s.workflows.graphs.iter().position(|g| g.name == "alpha").unwrap();
        s.graphs_delete();
        assert_eq!(s.workflows.graphs.len(), 1);
    }

    #[test]
    fn request_run_sets_run_request_for_nonempty_graph() {
        let mut s = state();
        s.request_run(); // empty graph -> no request
        assert!(s.run_request.is_none());
        let id = s.graph().unwrap().id;
        s.graph_mut().unwrap().add_node(NodeKind::GetLastAlert, 0, 0);
        s.request_run();
        assert_eq!(s.run_request, Some(id));
    }

    #[test]
    fn pan_and_zoom_clamp_to_world() {
        let mut s = state();
        let (_, _, w0, _) = s.viewport();
        s.zoom_in();
        let (_, _, w1, _) = s.viewport();
        assert!(w1 < w0);
        for _ in 0..50 {
            s.zoom_out();
        }
        let (_, _, w2, h2) = s.viewport();
        assert_eq!((w2, h2), (WF_GRID_W, WF_GRID_H));
        for _ in 0..100 {
            s.pan(-1, -1);
        }
        let (vx, vy, _, _) = s.viewport();
        assert_eq!((vx, vy), (0, 0));
    }
}
