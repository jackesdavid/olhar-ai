//! Workflows engine — topological run + per-graph trigger/concurrency logic.
//! Ported from the ratatui `src/workflows/engine.rs`; only the `ToolContext` and
//! `Alert` import paths change (web core).
//!
//! `run_dag` executes a whole graph once, fire-and-forget: source nodes fetch
//! their own data, reachable nodes run in topological order (each at most once,
//! resolving inputs from upstream outputs else config literals), stopping on the
//! first node error. The trigger/concurrency methods are the pure decision logic
//! the backend poller drives.
use super::domain::{ConcurrencyPolicy, Graph, NodeKind, EXEC_IN};
use super::node::{self, PortValue, PortValues};
use crate::agent::tools::{ToolContext, ToolUiEvent};
use crate::alerts::{Alert, AlertLevel};
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

/// Hard ceiling on node EXECUTIONS in a single run — the backstop against an infinite
/// loop (a `memoize=false` cycle / event storm). Far above any real acyclic graph, so it
/// never trips on a normal DAG. Exceeding it fails the run with a clear error.
pub const WF_MAX_STEPS: u32 = 10_000;
/// Wall-clock ceiling per run — covers `Wait`-heavy loops that stay under the step cap.
pub const WF_MAX_RUNTIME: Duration = Duration::from_secs(60);

/// Parse a `For each` **text** items value (config field or a wired text) into element
/// values. A JSON array (`["a","b"]` / `[1,2,3]`) maps each element to a typed `PortValue`
/// (string→Text, number→Num, bool→Bool, float/other→Text); any other text is split on
/// commas into `Text` elements. Empty → an empty list (the loop fires `completed`).
fn parse_text_items(text: &str) -> Vec<PortValue> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    if trimmed.starts_with('[') {
        if let Ok(serde_json::Value::Array(arr)) = serde_json::from_str::<serde_json::Value>(trimmed) {
            return arr.iter().map(json_to_port_value).collect();
        }
    }
    trimmed
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| PortValue::Text(s.to_string()))
        .collect()
}

fn json_to_port_value(v: &serde_json::Value) -> PortValue {
    match v {
        serde_json::Value::String(s) => PortValue::Text(s.clone()),
        serde_json::Value::Bool(b) => PortValue::Bool(*b),
        serde_json::Value::Number(n) => match n.as_i64() {
            Some(i) => PortValue::Num(i),
            None => PortValue::Text(n.to_string()), // floats: no float PortValue → text
        },
        other => PortValue::Text(other.to_string()),
    }
}

/// Run the whole graph once in topological order. Each node runs at most once; inputs
/// resolve from upstream outputs (wired edges) else config literals; stops on first error.
pub fn run_dag(graph: &Graph, sys: &ToolContext) -> Result<HashMap<u32, PortValues>, String> {
    run_dag_with(graph, sys, &mut |_| {}, &mut |_, _, _| {})
}

/// Like [`run_dag`], but calls `on_node(id)` just before each node executes (so the
/// shell can report the currently-running node) and, when the graph's `debug` flag
/// is on, `on_log(id, summary, elapsed_ms)` just after each node runs (`elapsed_ms`
/// is that node's own execution time).
pub fn run_dag_with(
    graph: &Graph,
    sys: &ToolContext,
    on_node: &mut dyn FnMut(u32),
    on_log: &mut dyn FnMut(u32, &str, u128),
) -> Result<HashMap<u32, PortValues>, String> {
    run_driver(graph, sys, true, &HashSet::new(), on_node, on_log, &mut |_| {})
}

/// Like [`run_dag_with`], but `emit(event)` is called whenever an `EmitEvent` node fires,
/// so the host can spawn a decoupled, guarded sub-run of the matching `OnEvent` subtree
/// (see [`run_subpath_eventful`]). `run_dag`/`run_dag_with` pass a no-op emit.
pub fn run_dag_eventful(
    graph: &Graph,
    sys: &ToolContext,
    on_node: &mut dyn FnMut(u32),
    on_log: &mut dyn FnMut(u32, &str, u128),
    emit: &mut dyn FnMut(&str),
) -> Result<HashMap<u32, PortValues>, String> {
    run_driver(graph, sys, true, &HashSet::new(), on_node, on_log, emit)
}

/// Run ONLY the subtree rooted at `entry` (an `OnEvent` node) — the body of a fired event,
/// as its own decoupled run. Loops + nested `EmitEvent`s inside the handler work the same.
pub fn run_subpath_eventful(
    graph: &Graph,
    sys: &ToolContext,
    entry: u32,
    on_node: &mut dyn FnMut(u32),
    on_log: &mut dyn FnMut(u32, &str, u128),
    emit: &mut dyn FnMut(&str),
) -> Result<HashMap<u32, PortValues>, String> {
    let mut forced = HashSet::new();
    forced.insert(entry);
    run_driver(graph, sys, false, &forced, on_node, on_log, emit)
}

/// Find the `OnEvent` node subscribing to `event`, if any (first match in node order).
pub fn on_event_node(graph: &Graph, event: &str) -> Option<u32> {
    graph.nodes.iter().find(|n| {
        n.kind == NodeKind::OnEvent
            && n.config.get("event").and_then(|v| v.as_str()) == Some(event)
    }).map(|n| n.id)
}

/// Multi-epoch work-list scheduler (replaces the old single `topo_order()` sweep).
///
/// Each epoch is a forward, settling Kahn pass over the **ordering edges** (all distinct
/// `(from,to)` pairs MINUS the exec back-edges) — so nodes run producer-before-consumer
/// exactly as `topo_order` did, but loops are possible: an exec back-edge that fires
/// re-enters its target on the NEXT epoch. `outputs`/`ran` persist across epochs (so
/// `memoize=true` nodes keep their value); a step + wall-clock budget bounds runaway loops.
/// `seed_entries` activates natural entry nodes (a normal run); `initial_forced` seeds a
/// sub-run at a specific node. With no back-edges + entry seeding this is a single epoch
/// identical to the old behavior.
#[allow(clippy::too_many_arguments)]
fn run_driver(
    graph: &Graph,
    sys: &ToolContext,
    seed_entries: bool,
    initial_forced: &HashSet<u32>,
    on_node: &mut dyn FnMut(u32),
    on_log: &mut dyn FnMut(u32, &str, u128),
    emit: &mut dyn FnMut(&str),
) -> Result<HashMap<u32, PortValues>, String> {
    let connected: HashSet<u32> = if graph.run_orphans {
        graph.nodes.iter().map(|n| n.id).collect()
    } else {
        graph.edges.iter().flat_map(|e| [e.from, e.to]).collect()
    };
    let back = graph.back_edges();

    // Ordering indegree + adjacency over DISTINCT non-back `(from,to)` pairs (same de-dup
    // as `topo_order`), so the forward graph is acyclic.
    let mut indeg_tmpl: HashMap<u32, usize> = graph.nodes.iter().map(|n| (n.id, 0)).collect();
    let mut adj: HashMap<u32, Vec<u32>> = graph.nodes.iter().map(|n| (n.id, Vec::new())).collect();
    let mut seen_pair = HashSet::new();
    for e in &graph.edges {
        if back.contains(&(e.from, e.to)) {
            continue;
        }
        if seen_pair.insert((e.from, e.to)) {
            if let Some(v) = adj.get_mut(&e.from) {
                v.push(e.to);
            }
            if let Some(d) = indeg_tmpl.get_mut(&e.to) {
                *d += 1;
            }
        }
    }

    let mut outputs: HashMap<u32, PortValues> = HashMap::new();
    let mut ran: HashSet<u32> = HashSet::new();
    let mut steps: u32 = 0;
    // Per-run iteration state for `ForEach` nodes (node id → next index), persisted across
    // epochs so each epoch advances the loop one element.
    let mut loop_state: HashMap<u32, usize> = HashMap::new();
    let run_started = Instant::now();

    let mut reentries = forward_epoch(
        graph, sys, &indeg_tmpl, &adj, &back, &connected, seed_entries, initial_forced,
        &mut outputs, &mut ran, &mut loop_state, &mut steps, run_started, on_node, on_log, emit,
    )?;
    // Later epochs: re-enter loops at the back-edge / ForEach targets until none fire.
    while !reentries.is_empty() {
        let forced: HashSet<u32> = reentries.into_iter().collect();
        reentries = forward_epoch(
            graph, sys, &indeg_tmpl, &adj, &back, &connected, false, &forced,
            &mut outputs, &mut ran, &mut loop_state, &mut steps, run_started, on_node, on_log, emit,
        )?;
    }
    Ok(outputs)
}

/// One forward Kahn pass over the ordering (non-back) edges. `seed_entries` activates the
/// natural entry nodes (epoch 0 only); `forced` activates loop re-entry targets. Returns
/// the back-edge targets that fired this epoch (the next epoch's re-entry points).
#[allow(clippy::too_many_arguments)]
fn forward_epoch(
    graph: &Graph,
    sys: &ToolContext,
    indeg_tmpl: &HashMap<u32, usize>,
    adj: &HashMap<u32, Vec<u32>>,
    back: &HashSet<(u32, u32)>,
    connected: &HashSet<u32>,
    seed_entries: bool,
    forced: &HashSet<u32>,
    outputs: &mut HashMap<u32, PortValues>,
    ran: &mut HashSet<u32>,
    loop_state: &mut HashMap<u32, usize>,
    steps: &mut u32,
    run_started: Instant,
    on_node: &mut dyn FnMut(u32),
    on_log: &mut dyn FnMut(u32, &str, u128),
    emit: &mut dyn FnMut(&str),
) -> Result<Vec<u32>, String> {
    let mut indeg = indeg_tmpl.clone();
    let mut settled: HashSet<u32> = HashSet::new();
    let mut live_out: HashSet<(u32, String)> = HashSet::new();
    let mut reentries: Vec<u32> = Vec::new();
    let total = graph.nodes.len();

    while settled.len() < total {
        // Earliest unsettled node with ordering-indegree 0 (the `topo_order` tie-break).
        let Some(nid) = graph
            .nodes
            .iter()
            .map(|n| n.id)
            .find(|id| !settled.contains(id) && indeg.get(id).copied().unwrap_or(0) == 0)
        else {
            break; // remaining nodes are reachable only via a back-edge not seeded this epoch
        };

        let settle = |settled: &mut HashSet<u32>, indeg: &mut HashMap<u32, usize>| {
            settled.insert(nid);
            if let Some(children) = adj.get(&nid) {
                for c in children {
                    if let Some(d) = indeg.get_mut(c) {
                        *d = d.saturating_sub(1);
                    }
                }
            }
        };

        if !connected.contains(&nid) {
            settle(&mut settled, &mut indeg);
            continue;
        }
        let n = graph.node(nid).ok_or("node vanished mid-run")?;

        // Activation: a forced re-entry target is active; otherwise a node with incoming
        // ORDERING exec edges activates when fed by a live one; a node with none is an
        // entry node, active only when we're seeding entries (epoch 0). An `If` lights one
        // branch, gating the other off — unchanged.
        let mut has_ord_exec = false;
        let mut fed_live = false;
        for e in graph
            .edges
            .iter()
            .filter(|e| e.to == nid && e.to_port == EXEC_IN.name && !back.contains(&(e.from, nid)))
        {
            has_ord_exec = true;
            if live_out.contains(&(e.from, e.from_port.clone())) {
                fed_live = true;
                break;
            }
        }
        let active = if n.kind == NodeKind::OnEvent {
            // An OnEvent entry NEVER runs on the normal trigger — only when its event fires
            // (the host seeds a sub-run with it in `forced`).
            forced.contains(&nid)
        } else if forced.contains(&nid) {
            true
        } else if has_ord_exec {
            fed_live
        } else {
            seed_entries
        };
        if !active {
            settle(&mut settled, &mut indeg);
            continue;
        }

        // Resolve inputs from wired upstream outputs (else config literals); a required
        // input wired to a source that never ran (gated off) → dead path, skip. (Every
        // ordering predecessor has settled by now, so `outputs` reflects which ran.)
        let mut inputs = PortValues::new();
        let mut dead = false;
        for port in n.kind.input_ports() {
            if let Some(edge) = graph.edges.iter().find(|e| e.to == nid && e.to_port == port.name) {
                match outputs.get(&edge.from).and_then(|m| m.get(&edge.from_port)) {
                    Some(v) => {
                        inputs.insert(port.name.to_string(), v.clone());
                    }
                    None if port.required && !outputs.contains_key(&edge.from) => {
                        dead = true;
                        break;
                    }
                    None => {}
                }
            } else if let Some(lit) = node::literal_for(&n.config, *port) {
                inputs.insert(port.name.to_string(), lit);
            }
        }
        if dead {
            settle(&mut settled, &mut indeg);
            continue;
        }

        // A node that already ran with memoize ON, re-entered by a loop: reuse its cached
        // output and STOP (don't re-publish exec) so a memoized loop terminates after one
        // pass. Warn so the operator knows to turn memoize off to actually iterate.
        // `ForEach` is exempt — it's the loop DRIVER and must re-run each epoch to advance.
        if ran.contains(&nid) && n.memoize && n.kind != NodeKind::ForEach {
            if graph.debug {
                on_log(nid, "memoize on for a node on a loop — it won't iterate (turn memoize off to loop)", 0);
            }
            settle(&mut settled, &mut indeg);
            continue;
        }

        if *steps >= WF_MAX_STEPS {
            return Err(format!("workflow exceeded step budget ({WF_MAX_STEPS} steps) — possible infinite loop"));
        }
        if run_started.elapsed() > WF_MAX_RUNTIME {
            return Err(format!("workflow exceeded time budget ({}s)", WF_MAX_RUNTIME.as_secs()));
        }

        on_node(nid);

        // ForEach: engine-driven iteration. Each epoch advances the index by one; fire
        // `loop_body` (with `item`+`index`) while elements remain and re-enter self next
        // epoch (so the body runs once per element, synchronously); fire `completed` once
        // the list is exhausted. No run_node — outputs are built here.
        if n.kind == NodeKind::ForEach {
            // Items come from a wired array (`List`), a wired text list, a single wired
            // value (one iteration), or the config text (CSV / JSON) when nothing is wired.
            let items: Vec<PortValue> = match inputs.get("items") {
                Some(PortValue::List(v)) => v.clone(),
                Some(PortValue::Text(t)) => parse_text_items(t),
                Some(other) => vec![other.clone()],
                None => parse_text_items(n.config.get("items").and_then(|v| v.as_str()).unwrap_or("")),
            };
            let i = loop_state.get(&nid).copied().unwrap_or(0);
            let mut o = PortValues::new();
            o.insert("index".into(), PortValue::Num(i as i64));
            if i < items.len() {
                o.insert("item".into(), items[i].clone());
                live_out.insert((nid, "loop_body".into()));
                loop_state.insert(nid, i + 1);
                reentries.push(nid); // self re-entry → next epoch runs the next element
            } else {
                live_out.insert((nid, "completed".into()));
                loop_state.remove(&nid);
            }
            *steps += 1;
            if graph.debug {
                on_log(nid, &node::debug_summary(n.kind, &n.config, &inputs, &o), 0);
            }
            outputs.insert(nid, o);
            ran.insert(nid);
            settle(&mut settled, &mut indeg);
            continue;
        }

        let started = Instant::now();
        // IO-bound kinds run under a per-node deadline (watchdog thread) so a stuck call
        // aborts the run instead of hanging forever; pure/instant kinds run inline.
        let o = match node::node_timeout(n.kind, &n.config) {
            Some(deadline) => node::run_node_timed(n.kind, &n.config, &inputs, sys, deadline),
            None => node::run_node(n.kind, &n.config, &inputs, sys),
        }
        .map_err(|e| format!("{}: {e}", n.kind.label()))?;
        let elapsed_ms = started.elapsed().as_millis();
        *steps += 1;

        if graph.debug {
            on_log(nid, &node::debug_summary(n.kind, &n.config, &inputs, &o), elapsed_ms);
        }

        if n.kind == NodeKind::ShowImage {
            if let Some(jpeg) =
                inputs.get("image").and_then(PortValue::as_image).and_then(|b| node::to_jpeg(b).ok())
            {
                let label = n.config.get("label").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let _ = sys.ui_tx.send(ToolUiEvent::ShowImage { graph: graph.id, node: nid, label, jpeg });
            }
        }

        // EmitEvent: fire the event so the host spawns the (guarded) handler sub-run.
        if n.kind == NodeKind::EmitEvent {
            if let Some(ev) = n.config.get("event").and_then(|v| v.as_str()).filter(|s| !s.is_empty()) {
                emit(ev);
            }
        }

        if n.kind == NodeKind::If {
            let cond = inputs.get("condition").and_then(PortValue::as_bool).unwrap_or(false);
            live_out.insert((nid, if cond { "true" } else { "false" }.to_string()));
        } else {
            for p in n.kind.exec_out_ports() {
                live_out.insert((nid, p.name.to_string()));
            }
        }
        outputs.insert(nid, o);
        ran.insert(nid);

        // Any live exec out pin firing along a back-edge re-enters that target next epoch.
        for e in graph
            .edges
            .iter()
            .filter(|e| e.from == nid && e.to_port == EXEC_IN.name && back.contains(&(nid, e.to)))
        {
            if live_out.contains(&(nid, e.from_port.clone())) && !reentries.contains(&e.to) {
                reentries.push(e.to);
            }
        }

        settle(&mut settled, &mut indeg);
    }
    Ok(reentries)
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

/// Per-(tenant, graph) trigger timers + concurrency counters + per-tenant new-alert
/// high-water mark. Keyed by `(tenant_id, graph_id)` because graph ids are per-tenant — a
/// single pod serving multiple tenants would otherwise collide graph 1 of tenant A with
/// graph 1 of tenant B.
#[derive(Default)]
pub struct WorkflowEngine {
    cron_last: HashMap<(uuid::Uuid, u32), Instant>,
    runs: HashMap<(uuid::Uuid, u32), GraphRun>,
    alert_seen_max: HashMap<uuid::Uuid, u64>,
}

impl WorkflowEngine {
    pub fn new() -> Self {
        Self::default()
    }

    /// Seed a tenant's new-alert high-water mark at boot (from the current alert-store max)
    /// so the existing backlog doesn't fire `OnNewAlert` graphs on startup.
    pub fn seed_alert_seen_max(&mut self, tenant: uuid::Uuid, max: u64) {
        self.alert_seen_max.insert(tenant, max);
    }

    /// Decide whether a fresh fire of `(tenant, graph_id)` should spawn, queue, or be skipped.
    pub fn decide(&self, tenant: uuid::Uuid, graph_id: u32, policy: &ConcurrencyPolicy) -> Fire {
        let r = self.runs.get(&(tenant, graph_id)).copied().unwrap_or_default();
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

    pub fn on_spawn(&mut self, tenant: uuid::Uuid, graph_id: u32) {
        self.runs.entry((tenant, graph_id)).or_default().running += 1;
    }

    pub fn on_queue(&mut self, tenant: uuid::Uuid, graph_id: u32) {
        self.runs.entry((tenant, graph_id)).or_default().queued += 1;
    }

    /// Call when a run finishes. Returns `true` if a queued fire should now spawn.
    pub fn on_finish(&mut self, tenant: uuid::Uuid, graph_id: u32) -> bool {
        let r = self.runs.entry((tenant, graph_id)).or_default();
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

    /// Total in-flight runs across all tenants/graphs (the global safety backstop reads this).
    pub fn total_running(&self) -> u32 {
        self.runs.values().map(|r| r.running).sum()
    }

    /// Seed a graph's cron timer to a prior fire instant (Phase 0.5 restore). The adapter
    /// computes this anchor from the persisted wall-clock fire time so a mid-interval restart
    /// waits out the remainder. Pure: persistence lives in `workflows::repo`.
    pub fn seed_cron_last(&mut self, tenant: uuid::Uuid, graph_id: u32, last: Instant) {
        self.cron_last.insert((tenant, graph_id), last);
    }

    /// A tenant's current new-alert high-water mark (0 if unseen) — so the adapter can
    /// detect when a tick advanced it and persist the new value.
    pub fn alert_seen_max(&self, tenant: uuid::Uuid) -> u64 {
        self.alert_seen_max.get(&tenant).copied().unwrap_or(0)
    }

    /// Cron: returns `true` (and re-arms the timer) when `every_secs` has elapsed.
    pub fn cron_due(&mut self, tenant: uuid::Uuid, graph_id: u32, every_secs: u64, now: Instant) -> bool {
        let key = (tenant, graph_id);
        let due = match self.cron_last.get(&key) {
            Some(last) => now.duration_since(*last) >= Duration::from_secs(every_secs),
            None => true,
        };
        if due {
            self.cron_last.insert(key, now);
        }
        due
    }

    /// Collect the levels of all NEW non-workflow alerts for `tenant` (id past its high-water
    /// mark) and advance that tenant's mark ONCE to the max id seen (so the backlog fires once
    /// and workflow-made alerts can't loop). Call this **once per tenant per tick**.
    pub fn drain_new_alert_levels(&mut self, tenant: uuid::Uuid, alerts: &[Alert]) -> Vec<AlertLevel> {
        let seen = self.alert_seen_max.get(&tenant).copied().unwrap_or(0);
        let mut levels = Vec::new();
        let mut max_id = seen;
        for a in alerts {
            if a.id > seen {
                max_id = max_id.max(a.id);
                if a.source != "workflow" {
                    levels.push(a.level);
                }
            }
        }
        self.alert_seen_max.insert(tenant, max_id);
        levels
    }

    /// Whether any of this tick's new alert levels is at or above `min_level`.
    pub fn levels_qualify(levels: &[AlertLevel], min_level: AlertLevel) -> bool {
        let min_rank = level_rank(min_level);
        levels.iter().any(|l| level_rank(*l) >= min_rank)
    }

    /// Single-graph convenience (drains + checks) for one tenant.
    pub fn new_alert_fires(&mut self, tenant: uuid::Uuid, alerts: &[Alert], min_level: AlertLevel) -> bool {
        let levels = self.drain_new_alert_levels(tenant, alerts);
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
        ToolContext::new_local(
            std::sync::Arc::new(std::sync::Mutex::new(vec![])),
            crate::db::Db::lazy("postgres://sentinel:sentinel@localhost/sentinel_test"),
            crate::tenancy::DEFAULT_TENANT_ID,
            std::sync::Arc::new(std::sync::Mutex::new(vec![])),
            std::sync::Arc::new(std::sync::Mutex::new(vec![])),
            None,
            tx,
        )
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
    fn if_node_runs_only_the_live_branch() {
        for (cond, want, skip) in [(true, "a", "b"), (false, "b", "a")] {
            let ctx = test_ctx();
            let mut g = Graph::new(1, "g");
            let gate = g.add_node(NodeKind::If, 0, 0);
            g.node_mut(gate).unwrap().config["condition"] = json!(cond); // literal Bool
            let a = g.add_node(NodeKind::Format, 0, 0);
            let b = g.add_node(NodeKind::Format, 0, 0);
            g.add_edge(gate, "true", a, "exec_in").unwrap();
            g.add_edge(gate, "false", b, "exec_in").unwrap();
            let outs = run_dag(&g, &ctx).unwrap();
            let chosen = if want == "a" { a } else { b };
            let skipped = if skip == "a" { a } else { b };
            assert!(outs.contains_key(&gate), "If itself runs");
            assert!(outs.contains_key(&chosen), "live branch runs (cond={cond})");
            assert!(!outs.contains_key(&skipped), "dead branch is skipped (cond={cond})");
        }
    }

    #[test]
    fn show_image_mirrors_its_input_frame_to_the_ui() {
        // A jpeg literal on the node's `image` port is read at run time; the engine
        // should then emit a ShowImage UI event tagged with that node's id.
        let path = std::env::temp_dir().join("wf_eng_show_image.jpg");
        let img = image::RgbImage::from_pixel(16, 12, image::Rgb([10, 200, 120]));
        image::DynamicImage::ImageRgb8(img)
            .save_with_format(&path, image::ImageFormat::Jpeg)
            .unwrap();

        let (tx, rx) = std::sync::mpsc::channel();
        let mut ctx = test_ctx();
        ctx.ui_tx = tx;
        let mut g = Graph::new(1, "g");
        let show = g.add_node(NodeKind::ShowImage, 0, 0);
        g.node_mut(show).unwrap().config = json!({ "label": "cam", "image": path.to_str().unwrap() });
        // run_orphans so the single, edgeless node participates in the run.
        g.run_orphans = true;
        run_dag(&g, &ctx).unwrap();

        match rx.try_recv() {
            Ok(ToolUiEvent::ShowImage { graph, node, label, jpeg }) => {
                assert_eq!(graph, g.id);
                assert_eq!(node, show);
                assert_eq!(label, "cam");
                assert!(!jpeg.is_empty());
            }
            _ => panic!("expected a ShowImage UI event"),
        }
    }

    #[test]
    fn exec_edge_without_if_still_runs_both() {
        // Regression: a plain exec edge (no branch) must not gate anything off.
        let ctx = test_ctx();
        let mut g = Graph::new(1, "g");
        let a = g.add_node(NodeKind::Format, 0, 0);
        let b = g.add_node(NodeKind::Format, 0, 0);
        g.add_edge(a, "exec_out", b, "exec_in").unwrap();
        let outs = run_dag(&g, &ctx).unwrap();
        assert!(outs.contains_key(&a) && outs.contains_key(&b));
    }

    #[test]
    fn fan_out_runs_the_source_once_and_both_consumers_see_its_value() {
        // One source (GetLastAlert) feeding two Format consumers: the source must run
        // exactly once (work-list settles it once) and both consumers read its output.
        let ctx = test_ctx();
        ctx.alerts.lock().unwrap().push(alert(1, AlertLevel::Info, "operator", "boom"));
        let mut g = Graph::new(1, "g");
        let gla = g.add_node(NodeKind::GetLastAlert, 0, 0);
        let f1 = g.add_node(NodeKind::Format, 0, 0);
        let f2 = g.add_node(NodeKind::Format, 0, 0);
        g.node_mut(f1).unwrap().config["template"] = json!("1:{p1}");
        g.node_mut(f2).unwrap().config["template"] = json!("2:{p1}");
        g.add_edge(gla, "message", f1, "p1").unwrap();
        g.add_edge(gla, "message", f2, "p1").unwrap();
        let outs = run_dag(&g, &ctx).unwrap();
        assert_eq!(outs[&f1]["text"].as_text(), Some("1:boom"));
        assert_eq!(outs[&f2]["text"].as_text(), Some("2:boom"));
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
    fn node_on_dead_branch_does_not_activate_or_error() {
        // Repro of the reported bug: If1 goes false and its (only) true branch is the
        // sole feeder of a *second* If's condition. The second If must NOT activate —
        // its required `condition` is wired from a node the false branch gated off — so
        // the run completes cleanly with no "missing required input 'condition'" error.
        let ctx = test_ctx();
        ctx.alerts.lock().unwrap().push(alert(1, AlertLevel::Info, "op", "hi"));
        let mut g = Graph::new(1, "Test Workflow");
        g.debug = true;

        let if1 = g.add_node(NodeKind::If, 0, 0);
        g.node_mut(if1).unwrap().config["condition"] = json!(false); // takes the false branch
        let gla = g.add_node(NodeKind::GetLastAlert, 0, 0); // lives on if1's true branch
        g.add_edge(if1, "true", gla, "exec_in").unwrap();

        let if2 = g.add_node(NodeKind::If, 0, 0); // the "other if"
        let pr = g.add_node(NodeKind::Print, 0, 0);
        g.add_edge(gla, "ok", if2, "condition").unwrap(); // condition from the gated-off node
        g.add_edge(if2, "true", pr, "exec_in").unwrap();

        let activated = std::cell::RefCell::new(Vec::<u32>::new());
        let res = run_dag_with(&g, &ctx, &mut |id| activated.borrow_mut().push(id), &mut |_, _, _| {});

        let outs = res.expect("run must not error on a dead-branch dependency");
        assert!(outs.contains_key(&if1), "if1 ran");
        assert!(!outs.contains_key(&gla), "gla is gated off (if1 took false)");
        assert!(!outs.contains_key(&if2), "if2 must NOT run — its condition source was gated off");
        assert!(!activated.borrow().contains(&if2), "if2 must not even signal activation");
        assert!(!outs.contains_key(&pr), "if2's downstream stays gated off");
    }

    #[test]
    fn exec_back_edge_loops_until_step_budget() {
        // A -exec-> B -exec(back)-> A, both memoize=false → infinite loop, bounded by the
        // step budget (it must error, not hang).
        let ctx = test_ctx();
        let mut g = Graph::new(1, "loop");
        let a = g.add_node(NodeKind::Format, 0, 0);
        let b = g.add_node(NodeKind::Format, 0, 0);
        g.node_mut(a).unwrap().memoize = false;
        g.node_mut(b).unwrap().memoize = false;
        g.add_edge(a, "exec_out", b, "exec_in").unwrap();
        g.add_edge(b, "exec_out", a, "exec_in").unwrap(); // exec back-edge (allowed)
        let err = run_dag(&g, &ctx).unwrap_err();
        assert!(err.contains("step budget"), "got: {err}");
    }

    #[test]
    fn memoized_node_on_a_loop_runs_once_and_does_not_error() {
        // Same loop but memoize ON (default): the re-entered node reuses its output and
        // stops — the run completes cleanly (warn only), no infinite loop.
        let ctx = test_ctx();
        let mut g = Graph::new(1, "loop");
        let a = g.add_node(NodeKind::Format, 0, 0);
        let b = g.add_node(NodeKind::Format, 0, 0);
        g.add_edge(a, "exec_out", b, "exec_in").unwrap();
        g.add_edge(b, "exec_out", a, "exec_in").unwrap();
        let outs = run_dag(&g, &ctx).expect("memoized loop must terminate");
        assert!(outs.contains_key(&a) && outs.contains_key(&b));
    }

    #[test]
    fn add_edge_allows_exec_back_edge_but_rejects_data_cycle() {
        let mut g = Graph::new(1, "g");
        let a = g.add_node(NodeKind::Format, 0, 0); // text out
        let b = g.add_node(NodeKind::Format, 0, 0);
        g.add_edge(a, "exec_out", b, "exec_in").unwrap();
        // exec back-edge → allowed (a loop).
        assert!(g.add_edge(b, "exec_out", a, "exec_in").is_ok());
        // data back-edge (b.text → a.p1) would create a data cycle → rejected.
        let err = g.add_edge(b, "text", a, "p1").unwrap_err();
        assert!(err.contains("data cycle"), "got: {err}");
    }

    // Run a graph capturing the node-activation order + per-node debug summaries (no DB).
    fn run_capture(g: &Graph, ctx: &ToolContext) -> (Vec<u32>, Vec<(u32, String)>) {
        let acts = std::cell::RefCell::new(Vec::<u32>::new());
        let logs = std::cell::RefCell::new(Vec::<(u32, String)>::new());
        run_dag_with(
            g,
            ctx,
            &mut |id| acts.borrow_mut().push(id),
            &mut |id, m, _| logs.borrow_mut().push((id, m.to_string())),
        )
        .unwrap();
        (acts.into_inner(), logs.into_inner())
    }

    #[test]
    fn for_each_iterates_in_order_then_fires_completed_once() {
        // ForEach("a,b,c") → loop_body → Format("{p1}", p1=item, memoize off)
        //                  → completed → Format("done").
        let ctx = test_ctx();
        let mut g = Graph::new(1, "loop");
        g.debug = true;
        let fe = g.add_node(NodeKind::ForEach, 0, 0);
        g.node_mut(fe).unwrap().config["items"] = json!("a,b,c");
        let body = g.add_node(NodeKind::Format, 0, 0);
        g.node_mut(body).unwrap().memoize = false; // re-run each iteration
        g.node_mut(body).unwrap().config["template"] = json!("{p1}");
        let done = g.add_node(NodeKind::Format, 0, 0);
        g.node_mut(done).unwrap().config["template"] = json!("done");
        g.add_edge(fe, "loop_body", body, "exec_in").unwrap();
        g.add_edge(fe, "item", body, "p1").unwrap();
        g.add_edge(fe, "completed", done, "exec_in").unwrap();

        let (acts, logs) = run_capture(&g, &ctx);
        assert_eq!(acts.iter().filter(|&&x| x == body).count(), 3, "body runs once per element");
        assert_eq!(acts.iter().filter(|&&x| x == done).count(), 1, "completed fires once");
        assert_eq!(acts.iter().filter(|&&x| x == fe).count(), 4, "fe: 3 elements + final");
        let body_logs: Vec<String> =
            logs.iter().filter(|(id, _)| *id == body).map(|(_, m)| m.clone()).collect();
        assert_eq!(body_logs, vec!["→ a", "→ b", "→ c"], "item flows in order");
    }

    #[test]
    fn for_each_empty_list_only_fires_completed() {
        let ctx = test_ctx();
        let mut g = Graph::new(1, "loop");
        let fe = g.add_node(NodeKind::ForEach, 0, 0); // items defaults to ""
        let body = g.add_node(NodeKind::Format, 0, 0);
        g.node_mut(body).unwrap().memoize = false;
        g.add_edge(fe, "loop_body", body, "exec_in").unwrap();
        let done = g.add_node(NodeKind::Format, 0, 0);
        g.add_edge(fe, "completed", done, "exec_in").unwrap();

        let (acts, _) = run_capture(&g, &ctx);
        assert_eq!(acts.iter().filter(|&&x| x == body).count(), 0, "no body on empty list");
        assert_eq!(acts.iter().filter(|&&x| x == done).count(), 1, "completed fires once");
    }

    #[test]
    fn for_each_iterates_a_json_number_array() {
        // A JSON number array yields Num elements (not text). Verify the body runs once per
        // element (3×) — the element type is checked directly in `parse_text_items_typing`.
        let ctx = test_ctx();
        let mut g = Graph::new(1, "loop");
        let fe = g.add_node(NodeKind::ForEach, 0, 0);
        g.node_mut(fe).unwrap().config["items"] = json!("[1, 2, 3]");
        let body = g.add_node(NodeKind::Print, 0, 0);
        g.node_mut(body).unwrap().memoize = false;
        g.add_edge(fe, "loop_body", body, "exec_in").unwrap();
        let (acts, _) = run_capture(&g, &ctx);
        assert_eq!(acts.iter().filter(|&&x| x == body).count(), 3, "3 JSON elements iterate");
    }

    #[test]
    fn parse_text_items_typing() {
        // CSV → Text elements; JSON array → typed (string→Text, number→Num, bool→Bool).
        let csv = parse_text_items("a, b ,c");
        assert_eq!(csv.len(), 3);
        assert_eq!(csv[0].as_text(), Some("a"));
        let jsonv = parse_text_items(r#"["x", 7, true]"#);
        assert_eq!(jsonv[0].as_text(), Some("x"));
        assert_eq!(jsonv[1].as_num(), Some(7));
        assert_eq!(jsonv[2].as_bool(), Some(true));
        assert!(parse_text_items("  ").is_empty());
    }


    #[test]
    fn emit_event_calls_the_sink_and_on_event_is_inert_on_a_normal_run() {
        let ctx = test_ctx();
        let mut g = Graph::new(1, "g");
        // EmitEvent("seen") (entry) → Print, so it's connected and runs on a normal trigger.
        let em = g.add_node(NodeKind::EmitEvent, 0, 0);
        g.node_mut(em).unwrap().config["event"] = json!("seen");
        let pr = g.add_node(NodeKind::Print, 0, 0);
        g.add_edge(em, "exec_out", pr, "exec_in").unwrap();
        // OnEvent("seen") → Format: must NOT run on a normal trigger.
        let on = g.add_node(NodeKind::OnEvent, 0, 0);
        g.node_mut(on).unwrap().config["event"] = json!("seen");
        let fmt = g.add_node(NodeKind::Format, 0, 0);
        g.add_edge(on, "exec_out", fmt, "exec_in").unwrap();

        let mut fired: Vec<String> = Vec::new();
        let outs = run_dag_eventful(&g, &ctx, &mut |_| {}, &mut |_, _, _| {}, &mut |ev| fired.push(ev.to_string())).unwrap();
        assert_eq!(fired, vec!["seen"], "EmitEvent calls the emit sink");
        assert!(outs.contains_key(&em), "EmitEvent ran");
        assert!(!outs.contains_key(&on), "OnEvent must NOT run on a normal trigger");
        assert!(!outs.contains_key(&fmt), "the OnEvent handler is inert on a normal run");

        // The handler runs only when its event fires (a sub-run seeded at the OnEvent node).
        let entry = on_event_node(&g, "seen").expect("OnEvent found by name");
        assert_eq!(entry, on);
        let outs2 = run_subpath_eventful(&g, &ctx, entry, &mut |_| {}, &mut |_, _, _| {}, &mut |_| {}).unwrap();
        assert!(outs2.contains_key(&on) && outs2.contains_key(&fmt), "handler subtree runs on its event");
        assert!(!outs2.contains_key(&em), "the sub-run runs ONLY the handler subtree");
    }

    #[test]
    fn concurrency_policies_spawn_queue_skip() {
        let t = uuid::Uuid::nil();
        let mut e = WorkflowEngine::new();
        let p = ConcurrencyPolicy::SkipIfRunning;
        assert_eq!(e.decide(t, 1, &p), Fire::Spawn);
        e.on_spawn(t, 1);
        assert_eq!(e.decide(t, 1, &p), Fire::Skip);
        assert!(!e.on_finish(t, 1));
        assert_eq!(e.decide(t, 1, &p), Fire::Spawn);

        let mut q = WorkflowEngine::new();
        let qp = ConcurrencyPolicy::QueueSingle { max_queued: 1 };
        assert_eq!(q.decide(t, 1, &qp), Fire::Spawn);
        q.on_spawn(t, 1);
        assert_eq!(q.decide(t, 1, &qp), Fire::Queue);
        q.on_queue(t, 1);
        assert_eq!(q.decide(t, 1, &qp), Fire::Skip);
        assert!(q.on_finish(t, 1));

        let mut c = WorkflowEngine::new();
        let cp = ConcurrencyPolicy::Concurrent { max_concurrent: 2 };
        assert_eq!(c.decide(t, 2, &cp), Fire::Spawn);
        c.on_spawn(t, 2);
        assert_eq!(c.decide(t, 2, &cp), Fire::Spawn);
        c.on_spawn(t, 2);
        assert_eq!(c.decide(t, 2, &cp), Fire::Skip);
    }

    #[test]
    fn same_graph_id_under_different_tenants_is_isolated() {
        // Graph id 1 of tenant A must not share run/cron state with graph id 1 of tenant B.
        let a = uuid::Uuid::from_u128(1);
        let b = uuid::Uuid::from_u128(2);
        let mut e = WorkflowEngine::new();
        let p = ConcurrencyPolicy::SkipIfRunning;
        e.on_spawn(a, 1);
        assert_eq!(e.decide(a, 1, &p), Fire::Skip, "A's graph 1 is running");
        assert_eq!(e.decide(b, 1, &p), Fire::Spawn, "B's graph 1 is independent");
        let base = Instant::now();
        e.seed_cron_last(a, 1, base);
        assert!(!e.cron_due(a, 1, 30, base + Duration::from_secs(10)));
        assert!(e.cron_due(b, 1, 30, base + Duration::from_secs(10)), "B's cron is independent");
    }

    #[test]
    fn new_alert_watcher_filters_and_advances_high_water() {
        let t = uuid::Uuid::nil();
        let alerts = vec![
            alert(5, AlertLevel::Error, "operator", "old"),
            alert(6, AlertLevel::Error, "workflow", "loop"),
            alert(7, AlertLevel::Info, "operator", "new info"),
            alert(8, AlertLevel::Debug, "operator", "noise"),
        ];
        let mut e = WorkflowEngine::new();
        e.seed_alert_seen_max(t, 5);
        assert!(e.new_alert_fires(t, &alerts, AlertLevel::Info));
        assert!(!e.new_alert_fires(t, &alerts, AlertLevel::Info));
        let mut e2 = WorkflowEngine::new();
        e2.seed_alert_seen_max(t, 5);
        assert!(!e2.new_alert_fires(t, &alerts, AlertLevel::Warn));
    }

    #[test]
    fn alert_seen_max_getter_tracks_drain() {
        let t = uuid::Uuid::nil();
        let alerts = vec![
            alert(5, AlertLevel::Error, "operator", "old"),
            alert(9, AlertLevel::Info, "operator", "new"),
        ];
        let mut e = WorkflowEngine::new();
        e.seed_alert_seen_max(t, 5);
        assert_eq!(e.alert_seen_max(t), 5, "seeded watermark");
        let _ = e.drain_new_alert_levels(t, &alerts);
        assert_eq!(e.alert_seen_max(t), 9, "watermark advanced to the max id seen");
    }

    #[test]
    fn seeded_cron_waits_out_remaining_interval() {
        // Phase 0.5: a restart seeds cron_last from the persisted fire time so a
        // 30s cron that fired mid-interval does NOT fire immediately on boot.
        let t = uuid::Uuid::nil();
        let mut e = WorkflowEngine::new();
        let base = Instant::now();
        e.seed_cron_last(t, 1, base);
        // 10s after the last fire → not due yet.
        assert!(!e.cron_due(t, 1, 30, base + Duration::from_secs(10)));
        // 40s after the last fire → due (and re-arms at that instant).
        assert!(e.cron_due(t, 1, 30, base + Duration::from_secs(40)));
        // Just after re-arming → not due again.
        assert!(!e.cron_due(t, 1, 30, base + Duration::from_secs(50)));
    }
}
