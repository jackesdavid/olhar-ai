//! Workflows engine — topological run + per-graph trigger/concurrency logic.
//! Ported from the ratatui `src/workflows/engine.rs`; only the `ToolContext` and
//! `Alert` import paths change (web core).
//!
//! `run_dag` executes a whole graph once, fire-and-forget: source nodes fetch
//! their own data, reachable nodes run in topological order (each at most once,
//! resolving inputs from upstream outputs else config literals), stopping on the
//! first node error. The trigger/concurrency methods are the pure decision logic
//! the backend poller drives.
use super::domain::{ConcurrencyPolicy, Graph};
use super::node::{self, PortValues};
use crate::agent::tools::ToolContext;
use crate::alerts::{Alert, AlertLevel};
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Run the whole graph once in topological order. Each node runs at most once; inputs
/// resolve from upstream outputs (wired edges) else config literals; stops on first error.
pub fn run_dag(graph: &Graph, sys: &ToolContext) -> Result<HashMap<u32, PortValues>, String> {
    run_dag_with(graph, sys, &mut |_| {})
}

/// Like [`run_dag`], but calls `on_node(id)` just before each node executes — used
/// by the shell to report the currently-running node for the live editor.
pub fn run_dag_with(
    graph: &Graph,
    sys: &ToolContext,
    on_node: &mut dyn FnMut(u32),
) -> Result<HashMap<u32, PortValues>, String> {
    let order = graph.topo_order()?;
    // Unless the graph opts in (`run_orphans`), only nodes wired into the graph
    // participate in a run — a node with no edges (in or out) is an orphan on the
    // canvas; running it would just do work whose output goes nowhere, so skip it
    // (and don't highlight it as executing).
    let connected: std::collections::HashSet<u32> = if graph.run_orphans {
        order.iter().copied().collect()
    } else {
        graph.edges.iter().flat_map(|e| [e.from, e.to]).collect()
    };
    let mut outputs: HashMap<u32, PortValues> = HashMap::new();
    for nid in order {
        if !connected.contains(&nid) {
            continue;
        }
        on_node(nid);
        let n = graph.node(nid).ok_or("node vanished mid-run")?;
        let mut inputs = PortValues::new();
        for port in n.kind.input_ports() {
            if let Some(edge) = graph.edges.iter().find(|e| e.to == nid && e.to_port == port.name) {
                // Wired: take the upstream node's output (already run, earlier in topo order).
                if let Some(v) = outputs.get(&edge.from).and_then(|m| m.get(&edge.from_port)) {
                    inputs.insert(port.name.to_string(), v.clone());
                }
            } else if let Some(lit) = node::literal_for(&n.config, *port) {
                inputs.insert(port.name.to_string(), lit);
            }
        }
        let o = node::run_node(n.kind, &n.config, &inputs, sys)
            .map_err(|e| format!("{}: {e}", n.kind.label()))?;
        outputs.insert(nid, o);
    }
    Ok(outputs)
}

/// What to do when a graph's trigger fires.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fire {
    Spawn,
    Queue,
    Skip,
}

#[derive(Default, Clone, Copy)]
struct GraphRun {
    running: u32,
    queued: u32,
}

/// Per-graph trigger timers + concurrency counters + the new-alert high-water mark.
pub struct WorkflowEngine {
    cron_last: HashMap<u32, Instant>,
    runs: HashMap<u32, GraphRun>,
    alert_seen_max: u64,
}

impl WorkflowEngine {
    /// `alert_seen_max` should be seeded from the current alert-store max at boot so the
    /// existing backlog doesn't fire `OnNewAlert` graphs on startup.
    pub fn new(alert_seen_max: u64) -> Self {
        WorkflowEngine { cron_last: HashMap::new(), runs: HashMap::new(), alert_seen_max }
    }

    /// Decide whether a fresh fire of `graph_id` should spawn, queue, or be skipped.
    pub fn decide(&self, graph_id: u32, policy: &ConcurrencyPolicy) -> Fire {
        let r = self.runs.get(&graph_id).copied().unwrap_or_default();
        match policy {
            ConcurrencyPolicy::SkipIfRunning => {
                if r.running == 0 {
                    Fire::Spawn
                } else {
                    Fire::Skip
                }
            }
            ConcurrencyPolicy::QueueSingle { max_queued } => {
                if r.running == 0 {
                    Fire::Spawn
                } else if r.queued < *max_queued {
                    Fire::Queue
                } else {
                    Fire::Skip
                }
            }
            ConcurrencyPolicy::Concurrent { max_concurrent } => {
                if r.running < (*max_concurrent).max(1) {
                    Fire::Spawn
                } else {
                    Fire::Skip
                }
            }
        }
    }

    pub fn on_spawn(&mut self, graph_id: u32) {
        self.runs.entry(graph_id).or_default().running += 1;
    }

    pub fn on_queue(&mut self, graph_id: u32) {
        self.runs.entry(graph_id).or_default().queued += 1;
    }

    /// Call when a run finishes. Returns `true` if a queued fire should now spawn.
    pub fn on_finish(&mut self, graph_id: u32) -> bool {
        let r = self.runs.entry(graph_id).or_default();
        if r.running > 0 {
            r.running -= 1;
        }
        if r.queued > 0 {
            r.queued -= 1;
            true
        } else {
            false
        }
    }

    /// Total in-flight runs across all graphs (the global safety backstop reads this).
    pub fn total_running(&self) -> u32 {
        self.runs.values().map(|r| r.running).sum()
    }

    /// Cron: returns `true` (and re-arms the timer) when `every_secs` has elapsed.
    pub fn cron_due(&mut self, graph_id: u32, every_secs: u64, now: Instant) -> bool {
        let due = match self.cron_last.get(&graph_id) {
            Some(last) => now.duration_since(*last) >= Duration::from_secs(every_secs),
            None => true,
        };
        if due {
            self.cron_last.insert(graph_id, now);
        }
        due
    }

    /// Collect the levels of all NEW non-workflow alerts (id past the high-water mark) and
    /// advance the high-water mark ONCE to the max id seen (so the backlog only fires once and
    /// workflow-made alerts can't loop). Call this **once per tick**.
    pub fn drain_new_alert_levels(&mut self, alerts: &[Alert]) -> Vec<AlertLevel> {
        let mut levels = Vec::new();
        let mut max_id = self.alert_seen_max;
        for a in alerts {
            if a.id > self.alert_seen_max {
                max_id = max_id.max(a.id);
                if a.source != "workflow" {
                    levels.push(a.level);
                }
            }
        }
        self.alert_seen_max = max_id;
        levels
    }

    /// Whether any of this tick's new alert levels is at or above `min_level`.
    pub fn levels_qualify(levels: &[AlertLevel], min_level: AlertLevel) -> bool {
        let min_rank = level_rank(min_level);
        levels.iter().any(|l| level_rank(*l) >= min_rank)
    }

    /// Single-graph convenience (drains + checks).
    pub fn new_alert_fires(&mut self, alerts: &[Alert], min_level: AlertLevel) -> bool {
        let levels = self.drain_new_alert_levels(alerts);
        Self::levels_qualify(&levels, min_level)
    }
}

fn level_rank(l: AlertLevel) -> usize {
    AlertLevel::ALL.iter().position(|x| *x == l).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::super::domain::{Graph, NodeKind};
    use super::*;
    use crate::alerts::{Alert, AlertLevel};
    use serde_json::json;

    fn test_ctx() -> ToolContext {
        let (tx, _rx) = std::sync::mpsc::channel();
        ToolContext {
            devices: std::sync::Arc::new(std::sync::Mutex::new(vec![])),
            devices_path: std::env::temp_dir().join("wf_eng_devices.json"),
            alerts: std::sync::Arc::new(std::sync::Mutex::new(vec![])),
            alerts_path: std::env::temp_dir().join("wf_eng_alerts.json"),
            notif: std::sync::Arc::new(std::sync::Mutex::new(vec![])),
            map_path: std::env::temp_dir().join("wf_eng_map.json"),
            notif_cfg: None,
            ui_tx: tx,
        }
    }

    fn alert(id: u64, level: AlertLevel, source: &str, msg: &str) -> Alert {
        Alert {
            id,
            level,
            message: msg.into(),
            room_id: None,
            room_name: None,
            source: source.into(),
            created_at: "2026-06-04T00:00:00Z".into(),
            deliveries: vec![],
        }
    }

    #[test]
    fn run_dag_resolves_wired_input_in_topo_order() {
        let ctx = test_ctx();
        ctx.alerts.lock().unwrap().push(alert(1, AlertLevel::Info, "operator", "hi there"));
        let mut g = Graph::new(1, "g");
        let gla = g.add_node(NodeKind::GetLastAlert, 0, 0);
        let fmt = g.add_node(NodeKind::Format, 0, 0);
        g.node_mut(fmt).unwrap().config["template"] = json!("ALERT: {p1}");
        g.add_edge(gla, "message", fmt, "p1").unwrap();
        let outs = run_dag(&g, &ctx).unwrap();
        assert_eq!(outs[&fmt]["text"].as_text(), Some("ALERT: hi there"));
    }

    #[test]
    fn run_dag_stops_on_missing_required_input_without_io() {
        let ctx = test_ctx();
        ctx.alerts.lock().unwrap().push(alert(1, AlertLevel::Info, "operator", "hi"));
        let mut g = Graph::new(1, "g");
        let gla = g.add_node(NodeKind::GetLastAlert, 0, 0);
        let ca = g.add_node(NodeKind::CreateAlert, 0, 0); // message "" -> missing
        g.add_edge(gla, "room", ca, "room").unwrap(); // connect so it isn't skipped
        let err = run_dag(&g, &ctx).unwrap_err();
        assert!(err.contains("missing required input 'message'"), "got: {err}");
        // create_alert errored before persisting — only the seed alert remains.
        assert_eq!(ctx.alerts.lock().unwrap().len(), 1);
    }

    #[test]
    fn run_dag_skips_disconnected_nodes() {
        let ctx = test_ctx();
        ctx.alerts.lock().unwrap().push(alert(1, AlertLevel::Info, "operator", "hi"));
        let mut g = Graph::new(1, "g");
        // A lone, fully-wired-up create_alert with a literal message would normally
        // persist an alert — but with no edges it's an orphan and must be skipped.
        let ca = g.add_node(NodeKind::CreateAlert, 0, 0);
        g.node_mut(ca).unwrap().config["message"] = json!("orphan should not fire");
        let outs = run_dag(&g, &ctx).unwrap();
        assert!(!outs.contains_key(&ca), "orphan node should not run");
        assert_eq!(ctx.alerts.lock().unwrap().len(), 1, "no alert from the orphan");

        // ...unless the graph opts in via run_orphans.
        g.run_orphans = true;
        let outs = run_dag(&g, &ctx).unwrap();
        assert!(outs.contains_key(&ca), "orphan runs when run_orphans is on");
        assert_eq!(ctx.alerts.lock().unwrap().len(), 2, "orphan created an alert");
    }

    #[test]
    fn concurrency_policies_spawn_queue_skip() {
        let mut e = WorkflowEngine::new(0);
        let p = ConcurrencyPolicy::SkipIfRunning;
        assert_eq!(e.decide(1, &p), Fire::Spawn);
        e.on_spawn(1);
        assert_eq!(e.decide(1, &p), Fire::Skip);
        assert!(!e.on_finish(1));
        assert_eq!(e.decide(1, &p), Fire::Spawn);

        let mut q = WorkflowEngine::new(0);
        let qp = ConcurrencyPolicy::QueueSingle { max_queued: 1 };
        assert_eq!(q.decide(1, &qp), Fire::Spawn);
        q.on_spawn(1);
        assert_eq!(q.decide(1, &qp), Fire::Queue);
        q.on_queue(1);
        assert_eq!(q.decide(1, &qp), Fire::Skip);
        assert!(q.on_finish(1));

        let mut c = WorkflowEngine::new(0);
        let cp = ConcurrencyPolicy::Concurrent { max_concurrent: 2 };
        assert_eq!(c.decide(2, &cp), Fire::Spawn);
        c.on_spawn(2);
        assert_eq!(c.decide(2, &cp), Fire::Spawn);
        c.on_spawn(2);
        assert_eq!(c.decide(2, &cp), Fire::Skip);
    }

    #[test]
    fn new_alert_watcher_filters_and_advances_high_water() {
        let alerts = vec![
            alert(5, AlertLevel::Error, "operator", "old"),
            alert(6, AlertLevel::Error, "workflow", "loop"),
            alert(7, AlertLevel::Info, "operator", "new info"),
            alert(8, AlertLevel::Debug, "operator", "noise"),
        ];
        let mut e = WorkflowEngine::new(5);
        assert!(e.new_alert_fires(&alerts, AlertLevel::Info));
        assert!(!e.new_alert_fires(&alerts, AlertLevel::Info));
        let mut e2 = WorkflowEngine::new(5);
        assert!(!e2.new_alert_fires(&alerts, AlertLevel::Warn));
    }
}
