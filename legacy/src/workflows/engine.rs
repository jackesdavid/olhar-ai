//! Workflows engine — topological run + per-graph trigger/concurrency logic (Phase 3).
//!
//! `run_dag` executes a whole graph once, fire-and-forget: a fire seeds nothing special
//! (source nodes fetch their own data), reachable nodes run in topological order, each at
//! most once, resolving inputs from upstream outputs (wired edges) or config literals, and
//! the run stops on the first node error. The trigger/concurrency methods are the pure
//! decision logic the App-level poller drives (the thread spawning + App wiring lands in
//! Phase 6).
#![allow(dead_code)]

use super::domain::{ConcurrencyPolicy, Graph};
use super::node::{self, PortValues};
use crate::agent::tools::ToolContext;
use crate::alerts::domain::Alert;
use crate::alerts::AlertLevel;
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Run the whole graph once in topological order. Each node runs at most once; inputs
/// resolve from upstream outputs (wired edges) else config literals; stops on first error.
/// Returns each node's outputs (in-memory; discarded by the caller after summarising).
pub fn run_dag(graph: &Graph, sys: &ToolContext) -> Result<HashMap<u32, PortValues>, String> {
    let order = graph.topo_order()?;
    let mut outputs: HashMap<u32, PortValues> = HashMap::new();
    for nid in order {
        let n = graph.node(nid).ok_or("node vanished mid-run")?;
        let mut inputs = PortValues::new();
        for port in n.kind.input_ports() {
            if let Some(edge) = graph
                .edges
                .iter()
                .find(|e| e.to == nid && e.to_port == port.name)
            {
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
        WorkflowEngine {
            cron_last: HashMap::new(),
            runs: HashMap::new(),
            alert_seen_max,
        }
    }

    /// Decide whether a fresh fire of `graph_id` should spawn, queue, or be skipped,
    /// per its `ConcurrencyPolicy` and the in-flight/queued counts.
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

    /// Call when a run finishes. Returns `true` if a queued fire should now spawn
    /// (the `QueueSingle` drain).
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

    /// Cron: returns `true` (and re-arms the timer) when `every_secs` has elapsed since
    /// the last fire for this graph.
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
    /// workflow-made alerts can't loop). Call this **once per tick** — every `OnNewAlert` graph
    /// then decides for itself via `levels_qualify(&levels, min_level)`. This keeps multiple
    /// `OnNewAlert` graphs from starving each other on a shared watermark.
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

    /// Single-graph convenience (drains + checks). The App poller uses
    /// `drain_new_alert_levels` once per tick + `levels_qualify` per graph instead.
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
    use super::*;
    use super::super::domain::{Graph, NodeKind};
    use crate::alerts::domain::Alert;
    use crate::alerts::AlertLevel;
    use serde_json::json;
    use std::sync::{Arc, Mutex};

    fn test_ctx() -> ToolContext {
        let (tx, _rx) = std::sync::mpsc::channel();
        ToolContext {
            store: Arc::new(Mutex::new(vec![])),
            ui_tx: tx,
            notif: Arc::new(Mutex::new(vec![])),
            alerts: Arc::new(Mutex::new(vec![])),
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
        // get_last_alert.message -> format.p1 ; format("ALERT: {p1}")
        let ctx = test_ctx();
        ctx.alerts
            .lock()
            .unwrap()
            .push(alert(1, AlertLevel::Info, "operator", "hi there"));
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
        let mut g = Graph::new(1, "g");
        g.add_node(NodeKind::CreateAlert, 0, 0); // default message "" -> missing
        let err = run_dag(&g, &ctx).unwrap_err();
        assert!(err.contains("missing required input 'message'"), "got: {err}");
        assert!(ctx.alerts.lock().unwrap().is_empty(), "no alert on error");
    }

    #[test]
    fn concurrency_skip_if_running() {
        let mut e = WorkflowEngine::new(0);
        let p = ConcurrencyPolicy::SkipIfRunning;
        assert_eq!(e.decide(1, &p), Fire::Spawn);
        e.on_spawn(1);
        assert_eq!(e.decide(1, &p), Fire::Skip);
        assert!(!e.on_finish(1)); // nothing queued
        assert_eq!(e.decide(1, &p), Fire::Spawn);
    }

    #[test]
    fn concurrency_queue_single() {
        let mut e = WorkflowEngine::new(0);
        let p = ConcurrencyPolicy::QueueSingle { max_queued: 1 };
        assert_eq!(e.decide(1, &p), Fire::Spawn);
        e.on_spawn(1);
        assert_eq!(e.decide(1, &p), Fire::Queue);
        e.on_queue(1);
        assert_eq!(e.decide(1, &p), Fire::Skip);
        assert!(e.on_finish(1), "queued fire should drain to a spawn");
    }

    #[test]
    fn concurrency_concurrent_caps() {
        let mut e = WorkflowEngine::new(0);
        let p = ConcurrencyPolicy::Concurrent { max_concurrent: 2 };
        assert_eq!(e.decide(2, &p), Fire::Spawn);
        e.on_spawn(2);
        assert_eq!(e.decide(2, &p), Fire::Spawn);
        e.on_spawn(2);
        assert_eq!(e.decide(2, &p), Fire::Skip);
    }

    #[test]
    fn new_alert_watcher_filters_and_advances_high_water() {
        let alerts = vec![
            alert(5, AlertLevel::Error, "operator", "old"), // not > 5
            alert(6, AlertLevel::Error, "workflow", "loop"), // ignored: workflow source
            alert(7, AlertLevel::Info, "operator", "new info"),
            alert(8, AlertLevel::Debug, "operator", "noise"),
        ];
        // min Info: id7 qualifies -> fire once, then high-water at 8 -> no refire.
        let mut e = WorkflowEngine::new(5);
        assert!(e.new_alert_fires(&alerts, AlertLevel::Info));
        assert!(!e.new_alert_fires(&alerts, AlertLevel::Info));
        // min Warn: id6 is Error but workflow-sourced (ignored); nothing else ≥ Warn -> no fire.
        let mut e2 = WorkflowEngine::new(5);
        assert!(!e2.new_alert_fires(&alerts, AlertLevel::Warn));
    }
}
