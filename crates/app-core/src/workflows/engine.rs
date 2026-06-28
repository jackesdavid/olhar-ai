//! Workflows engine — topological run + per-graph trigger/concurrency logic.
//! Ported from the ratatui `src/workflows/engine.rs`; only the `ToolContext` and
//! `Alert` import paths change (web core).
//!
//! `run_dag` executes a whole graph once, fire-and-forget: source nodes fetch
//! their own data, reachable nodes run in topological order (each at most once,
//! resolving inputs from upstream outputs else config literals), stopping on the
//! first node error. The trigger/concurrency methods are the pure decision logic
//! the backend poller drives.
use base64::Engine as _;
use super::domain::{camera_switch_arms, string_switch_arms, ConcurrencyPolicy, Graph, NodeKind, PortType, EXEC_IN};
use super::repo;
use super::node::{self, PortValue, PortValues};
use crate::agent::tools::{ToolContext, ToolUiEvent};
use crate::alerts::{Alert, AlertLevel};
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

/// Hard ceiling on node EXECUTIONS in a single run — the backstop against an infinite
/// loop (a `memoize=false` cycle / event storm). Far above any real acyclic graph, so it
/// never trips on a normal DAG. Exceeding it fails the run with a clear error.
pub const WF_MAX_STEPS: u32 = 10_000;

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
    run_driver(graph, sys, true, &HashSet::new(), &PortValues::new(), on_node, on_log, &mut |_, _| {})
}

/// Like [`run_dag_with`], but `emit(event)` is called whenever an `EmitEvent` node fires,
/// so the host can spawn a decoupled, guarded sub-run of the matching `OnEvent` subtree
/// (see [`run_subpath_eventful`]). `run_dag`/`run_dag_with` pass a no-op emit.
pub fn run_dag_eventful(
    graph: &Graph,
    sys: &ToolContext,
    on_node: &mut dyn FnMut(u32),
    on_log: &mut dyn FnMut(u32, &str, u128),
    emit: &mut dyn FnMut(&str, &PortValues),
) -> Result<HashMap<u32, PortValues>, String> {
    run_driver(graph, sys, true, &HashSet::new(), &PortValues::new(), on_node, on_log, emit)
}

/// Run ONLY the subtree rooted at `entry` (an `OnEvent` node) — the body of a fired event,
/// as its own decoupled run. `entry_payload` seeds the `OnEvent`'s outputs (the event's
/// typed payload), so downstream nodes read each field by port name. Loops + nested
/// `EmitEvent`s inside the handler work the same.
pub fn run_subpath_eventful(
    graph: &Graph,
    sys: &ToolContext,
    entry: u32,
    entry_payload: PortValues,
    on_node: &mut dyn FnMut(u32),
    on_log: &mut dyn FnMut(u32, &str, u128),
    emit: &mut dyn FnMut(&str, &PortValues),
) -> Result<HashMap<u32, PortValues>, String> {
    let mut forced = HashSet::new();
    forced.insert(entry);
    run_driver(graph, sys, false, &forced, &entry_payload, on_node, on_log, emit)
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
    entry_payload: &PortValues,
    on_node: &mut dyn FnMut(u32),
    on_log: &mut dyn FnMut(u32, &str, u128),
    emit: &mut dyn FnMut(&str, &PortValues),
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
    let mut reentries = forward_epoch(
        graph, sys, &indeg_tmpl, &adj, &back, &connected, seed_entries, initial_forced, entry_payload,
        &mut outputs, &mut ran, &mut loop_state, &mut steps, on_node, on_log, emit,
    )?;
    // Later epochs: re-enter loops at the back-edge / ForEach targets until none fire.
    while !reentries.is_empty() {
        let forced: HashSet<u32> = reentries.into_iter().collect();
        reentries = forward_epoch(
            graph, sys, &indeg_tmpl, &adj, &back, &connected, false, &forced, entry_payload,
            &mut outputs, &mut ran, &mut loop_state, &mut steps, on_node, on_log, emit,
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
    entry_payload: &PortValues,
    outputs: &mut HashMap<u32, PortValues>,
    ran: &mut HashSet<u32>,
    loop_state: &mut HashMap<u32, usize>,
    steps: &mut u32,
    on_node: &mut dyn FnMut(u32),
    on_log: &mut dyn FnMut(u32, &str, u128),
    emit: &mut dyn FnMut(&str, &PortValues),
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
        if n.kind == NodeKind::Comment {
            settle(&mut settled, &mut indeg);
            continue;
        }

        // Activation: a forced re-entry target is active; otherwise a node with incoming
        // ORDERING exec edges activates when fed by a live one; a node with none is an
        // entry node, active only when we're seeding entries (epoch 0). An `If` lights one
        // branch, gating the other off — unchanged.
        let mut has_ord_exec = false;
        let mut fed_live = false;
        for e in graph.edges
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
        } else if n.kind == NodeKind::GetVariable {
            // A variable read re-evaluates every epoch (pure, no exec propagation) so a
            // consumer sees the value as of when it runs — after any upstream `Set` — rather
            // than a stale epoch-0 read. Cheap; never extends the run.
            true
        } else if has_ord_exec {
            fed_live
        } else {
            // No exec_in wired: pure data/logic nodes and true entry orphans. For nodes
            // with upstream data edges (comparison, conversion, etc.), activate as soon as
            // any upstream producer has run — they're pulled by the exec chain, not seeded.
            // True orphans (no incoming ordering edges at all) fall back to seed_entries.
            let has_upstream = graph.edges
                .iter()
                .any(|e| e.to == nid && !back.contains(&(e.from, nid)));
            if has_upstream {
                graph.edges
                    .iter()
                    .filter(|e| e.to == nid && !back.contains(&(e.from, nid)))
                    .any(|e| outputs.contains_key(&e.from))
            } else {
                seed_entries
            }
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
        // Node-aware ports: most kinds are static, but EmitEvent's inputs are its
        // configured event's payload fields (one per field). Gather wired values by name.
        for port in n.input_ports() {
            if let Some(edge) = graph.edges.iter().find(|e| e.to == nid && e.to_port == port.name) {
                match outputs.get(&edge.from).and_then(|m| m.get(&edge.from_port)) {
                    Some(v) => {
                        inputs.insert(port.name.clone(), v.clone());
                    }
                    None if port.required && !outputs.contains_key(&edge.from) => {
                        dead = true;
                        break;
                    }
                    None => {}
                }
            } else if port.ty == PortType::Camera {
                // Camera literals require device resolution — handled here where sys is available.
                if let Some(cam) = node::resolve_camera_literal(&n.config, &port.name, sys) {
                    inputs.insert(port.name.clone(), cam);
                }
            } else if let Some(lit) = node::literal_for_named(&n.config, &port.name, port.ty) {
                inputs.insert(port.name.clone(), lit);
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

        // FlipFlop: read cross-run state, fire `a` or `b`, write toggled state back.
        // Engine-driven (like ForEach) — no run_node call.
        if n.kind == NodeKind::FlipFlop {
            let state = repo::get_node_state(&sys.db, sys.tenant_id, graph.id, nid);
            // State stores the branch to fire next. Missing = first run → fire A (is_a=true).
            let is_a = state
                .as_ref()
                .and_then(|v| v.get("is_a"))
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            repo::set_node_state(
                &sys.db,
                sys.tenant_id,
                graph.id,
                nid,
                &serde_json::json!({ "is_a": !is_a }),
            );
            live_out.insert((nid, if is_a { "a" } else { "b" }.to_string()));
            let mut o = PortValues::new();
            o.insert("is_a".into(), PortValue::Bool(is_a));
            *steps += 1;
            if graph.debug {
                on_log(nid, &format!("Flip Flop → {}", if is_a { "a" } else { "b" }), 0);
            }
            outputs.insert(nid, o);
            ran.insert(nid);
            settle(&mut settled, &mut indeg);
            continue;
        }

        // Cooldown: pass exec through only when `window_secs` have elapsed since last fire.
        // Stores last-fired Unix-ms in node state; first run always passes.
        if n.kind == NodeKind::Cooldown {
            let window_secs = n.config
                .get("window_secs")
                .and_then(|v| v.as_u64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok())))
                .unwrap_or(60);
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            let state = repo::get_node_state(&sys.db, sys.tenant_id, graph.id, nid);
            let last_ms = state
                .as_ref()
                .and_then(|v| v.get("last_ms"))
                .and_then(|v| v.as_u64());
            let elapsed_secs = match last_ms {
                None => window_secs + 1, // never fired → always open
                Some(t) => now_ms.saturating_sub(t) / 1000,
            };
            let ok = elapsed_secs >= window_secs;
            if ok {
                repo::set_node_state(
                    &sys.db,
                    sys.tenant_id,
                    graph.id,
                    nid,
                    &serde_json::json!({ "last_ms": now_ms }),
                );
                live_out.insert((nid, "exec_out".to_string()));
            }
            let mut o = PortValues::new();
            o.insert("elapsed_secs".into(), PortValue::Num(elapsed_secs as i64));
            o.insert("ok".into(), PortValue::Bool(ok));
            *steps += 1;
            if graph.debug {
                on_log(nid, &format!("Cooldown: {elapsed_secs}s elapsed / {window_secs}s window → {}", if ok { "pass" } else { "blocked" }), 0);
            }
            outputs.insert(nid, o);
            ran.insert(nid);
            settle(&mut settled, &mut indeg);
            continue;
        }

        // TimeGate: debounced countdown with extension on re-trigger.
        //
        // First arm: write state=armed + expires_at, then poll every 250 ms until expiry
        // (holding this thread, just like Wait). When the countdown expires, write
        // state=idle and fire exec "out".
        //
        // Re-trigger while armed: extend expires_at if remaining < extend_secs, then
        // return immediately with no exec output — the keeper run is still polling.
        if n.kind == NodeKind::TimeGate {
            use chrono::Utc;
            let wait_secs = n.config.get("wait_secs")
                .and_then(|v| v.as_i64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok())))
                .unwrap_or(10);
            let extend_secs = n.config.get("extend_secs")
                .and_then(|v| v.as_i64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok())))
                .unwrap_or(wait_secs);
            let now_ts = Utc::now().timestamp();
            let state = repo::get_node_state(&sys.db, sys.tenant_id, graph.id, nid);
            let is_armed = state.as_ref()
                .and_then(|v| v.get("state"))
                .and_then(|v| v.as_str())
                .map(|s| s == "armed")
                .unwrap_or(false);

            let gate_key = (sys.tenant_id, graph.id, nid);
            let keeper_alive = sys.active_gates.lock().unwrap().contains(&gate_key);

            if is_armed && keeper_alive {
                // Re-trigger: a keeper is running. Extend expires_at if needed, then return.
                let expires_ts = state.as_ref()
                    .and_then(|v| v.get("expires_at"))
                    .and_then(|v| v.as_str())
                    .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                    .map(|t| t.timestamp())
                    .unwrap_or(now_ts + wait_secs);
                let remaining = expires_ts - now_ts;
                if remaining < extend_secs {
                    let new_expires_ts = now_ts + extend_secs;
                    repo::set_node_state(&sys.db, sys.tenant_id, graph.id, nid, &serde_json::json!({
                        "state": "armed",
                        "expires_at": chrono::DateTime::from_timestamp(new_expires_ts, 0)
                            .unwrap_or_else(Utc::now)
                            .to_rfc3339(),
                    }));
                    if graph.debug {
                        on_log(nid, &format!("Time Gate: extended +{}s", extend_secs - remaining), 0);
                    }
                } else if graph.debug {
                    on_log(nid, &format!("Time Gate: re-trigger, {remaining}s remaining (no extend)"), 0);
                }
                *steps += 1;
                outputs.insert(nid, PortValues::new());
                ran.insert(nid);
                settle(&mut settled, &mut indeg);
                continue;
            }

            // Idle, OR armed with a dead keeper (e.g. flow restarted) — this run becomes the keeper.
            // Arm fresh: if already armed just update the expiry so the keeper starts from now.
            let expires_ts = now_ts + wait_secs;
            repo::set_node_state(&sys.db, sys.tenant_id, graph.id, nid, &serde_json::json!({
                "state": "armed",
                "expires_at": chrono::DateTime::from_timestamp(expires_ts, 0)
                    .unwrap_or_else(Utc::now)
                    .to_rfc3339(),
            }));
            sys.active_gates.lock().unwrap().insert(gate_key);
            if graph.debug {
                if is_armed {
                    on_log(nid, &format!("Time Gate: keeper recovered, re-armed for {wait_secs}s"), 0);
                } else {
                    on_log(nid, &format!("Time Gate: armed for {wait_secs}s"), 0);
                }
            }

            let fired = loop {
                std::thread::sleep(Duration::from_millis(250));
                let st = repo::get_node_state(&sys.db, sys.tenant_id, graph.id, nid);
                let armed = st.as_ref()
                    .and_then(|v| v.get("state"))
                    .and_then(|v| v.as_str())
                    .map(|s| s == "armed")
                    .unwrap_or(false);
                if !armed {
                    break false; // reset by an external write
                }
                let exp = st.as_ref()
                    .and_then(|v| v.get("expires_at"))
                    .and_then(|v| v.as_str())
                    .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                    .map(|t| t.timestamp())
                    .unwrap_or(0);
                if Utc::now().timestamp() >= exp {
                    break true;
                }
            };

            sys.active_gates.lock().unwrap().remove(&gate_key);
            repo::set_node_state(&sys.db, sys.tenant_id, graph.id, nid,
                &serde_json::json!({ "state": "idle" }));
            if fired {
                live_out.insert((nid, "out".to_string()));
                if graph.debug {
                    on_log(nid, "Time Gate: countdown expired → out", 0);
                }
            } else if graph.debug {
                on_log(nid, "Time Gate: reset → no output", 0);
            }
            *steps += 1;
            outputs.insert(nid, PortValues::new());
            ran.insert(nid);
            settle(&mut settled, &mut indeg);
            continue;
        }

        let started = Instant::now();
        // IO-bound kinds run under a per-node deadline (watchdog thread) so a stuck call
        // aborts the run instead of hanging forever; pure/instant kinds run inline.
        let mut o = match node::node_timeout(n.kind, &n.config) {
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
                // Persist the frame so it survives browser refresh and flow restarts.
                repo::set_node_state(
                    &sys.db,
                    sys.tenant_id,
                    graph.id,
                    nid,
                    &serde_json::json!({
                        "jpeg_b64": base64::engine::general_purpose::STANDARD.encode(&jpeg),
                        "label": &label,
                    }),
                );
                let _ = sys.ui_tx.send(ToolUiEvent::ShowImage { graph: graph.id, node: nid, label, jpeg });
            }
        }

        // EmitEvent: fire the event so the host spawns the (guarded) handler sub-run.
        // The node's gathered inputs ARE the event's typed payload (one per field).
        if n.kind == NodeKind::EmitEvent {
            if let Some(ev) = n.config.get("event").and_then(|v| v.as_str()).filter(|s| !s.is_empty()) {
                emit(ev, &inputs);
            }
        }

        // OnEvent entry: seed its outputs with the delivered payload so downstream nodes
        // read each field by port name (run_node returned an empty map for it).
        // Camera-typed fields are upgraded from raw Text (IP/id from the wire) to CameraEntity.
        if n.kind == NodeKind::OnEvent {
            o = node::seed_event_payload(entry_payload, &n.output_ports(), sys);
        }

        if n.kind == NodeKind::If {
            let cond = inputs.get("condition").and_then(PortValue::as_bool).unwrap_or(false);
            live_out.insert((nid, if cond { "true" } else { "false" }.to_string()));
        } else if n.kind == NodeKind::AskAi {
            // AskAi: run_node computed the answer and stored it in `o["result"]`.
            let answer = o.get("result").and_then(PortValue::as_bool).unwrap_or(false);
            live_out.insert((nid, if answer { "true" } else { "false" }.to_string()));
        } else if n.kind == NodeKind::StringSwitch {
            let val = inputs.get("value").and_then(PortValue::as_text).unwrap_or_default();
            let arms = string_switch_arms(&n.config);
            if arms.iter().any(|a| a == val) {
                live_out.insert((nid, val.to_string()));
            } else {
                live_out.insert((nid, "default".to_string()));
            }
        } else if n.kind == NodeKind::CameraSwitch {
            let cam = inputs.get("camera").and_then(PortValue::as_camera);
            let port = if let Some(cam) = cam {
                let arms = camera_switch_arms(&n.config);
                let idx = arms.iter().position(|a| !a.is_empty() && (a == &cam.ip || a == &cam.id));
                idx.map(|i| format!("arm_{}", i + 1)).unwrap_or_else(|| "default".to_string())
            } else {
                "default".to_string()
            };
            live_out.insert((nid, port));
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
    use super::super::domain::{Graph, NodeKind, PortType};
    use super::*;
    use crate::alerts::{Alert, AlertLevel};
    use crate::ports::Vision;
    use crate::agent::detector::{PersonDetection, PersonResult};
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

    /// A Vision stub whose `ask_yes_no` returns a fixed answer. Used to test `AskAi`
    /// routing without hitting the Anthropic API.
    struct StubVision(bool);
    impl Vision for StubVision {
        fn detect_persons(&self, _: &[u8]) -> Result<PersonResult, String> { Ok(PersonResult { person: false, count: 0, confidence: 0.0 }) }
        fn detect_persons_detailed(&self, _: &[u8]) -> Result<Vec<PersonDetection>, String> { Ok(vec![]) }
        fn detect_objects_detailed(&self, _: &[u8], _: u32) -> Result<Vec<PersonDetection>, String> { Ok(vec![]) }
        fn describe(&self, _: &[u8], _: &str) -> Result<String, String> { Ok("stub".into()) }
        fn ask_yes_no(&self, _: &str, _: Option<&[u8]>) -> Result<bool, String> { Ok(self.0) }
    }

    fn ctx_with_vision(answer: bool) -> ToolContext {
        let (tx, _rx) = std::sync::mpsc::channel();
        let mut ctx = test_ctx();
        ctx.ui_tx = tx;
        ctx.vision = std::sync::Arc::new(StubVision(answer));
        ctx
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
        // Same loop but memoize ON (explicit): the re-entered node reuses its output and
        // stops — the run completes cleanly (warn only), no infinite loop.
        let ctx = test_ctx();
        let mut g = Graph::new(1, "loop");
        let a = g.add_node(NodeKind::Format, 0, 0);
        let b = g.add_node(NodeKind::Format, 0, 0);
        g.node_mut(a).unwrap().memoize = true;
        g.node_mut(b).unwrap().memoize = true;
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
    fn set_variable_then_get_variable_reads_the_value() {
        // Format("hello") --exec--> SetVariable(greeting)   [value <- Format.text]
        //                            SetVariable --exec--> Format("{p1}")  [p1 <- GetVariable.value]
        // The Get must read "hello" (the value Set wrote), not its epoch-0 zero default.
        let ctx = test_ctx();
        let mut g = Graph::new(1, "vars");
        let src = g.add_node(NodeKind::Format, 0, 0);
        g.node_mut(src).unwrap().config["template"] = json!("hello");
        let set = g.add_node(NodeKind::SetVariable, 0, 0);
        g.node_mut(set).unwrap().config["variable"] = json!("greeting");
        g.node_mut(set).unwrap().config["type"] = json!("text");
        let get = g.add_node(NodeKind::GetVariable, 0, 0);
        g.node_mut(get).unwrap().config["variable"] = json!("greeting");
        g.node_mut(get).unwrap().config["type"] = json!("text");
        let sink = g.add_node(NodeKind::Format, 0, 0);
        g.node_mut(sink).unwrap().config["template"] = json!("{p1}");
        g.add_edge(src, "exec_out", set, "exec_in").unwrap();
        g.add_edge(src, "text", set, "value").unwrap();
        g.add_edge(set, "exec_out", sink, "exec_in").unwrap();
        g.add_edge(get, "value", sink, "p1").unwrap();

        let outs = run_dag(&g, &ctx).expect("run");
        let got = outs.get(&sink).and_then(|m| m.get("text")).and_then(PortValue::as_text);
        assert_eq!(got, Some("hello"), "Get reads the value Set wrote");
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
        let outs = run_dag_eventful(&g, &ctx, &mut |_| {}, &mut |_, _, _| {}, &mut |ev, _| fired.push(ev.to_string())).unwrap();
        assert_eq!(fired, vec!["seen"], "EmitEvent calls the emit sink");
        assert!(outs.contains_key(&em), "EmitEvent ran");
        assert!(!outs.contains_key(&on), "OnEvent must NOT run on a normal trigger");
        assert!(!outs.contains_key(&fmt), "the OnEvent handler is inert on a normal run");

        // The handler runs only when its event fires (a sub-run seeded at the OnEvent node).
        let entry = on_event_node(&g, "seen").expect("OnEvent found by name");
        assert_eq!(entry, on);
        let outs2 = run_subpath_eventful(&g, &ctx, entry, PortValues::new(), &mut |_| {}, &mut |_, _, _| {}, &mut |_, _| {}).unwrap();
        assert!(outs2.contains_key(&on) && outs2.contains_key(&fmt), "handler subtree runs on its event");
        assert!(!outs2.contains_key(&em), "the sub-run runs ONLY the handler subtree");
    }

    #[test]
    fn event_payload_makes_dynamic_ports_and_seeds_the_on_event_output() {
        let ctx = test_ctx();
        let mut g = Graph::new(1, "g");
        // An event "seen" carrying a Text field "msg" — baked onto both nodes' config.
        let schema = json!({ "event": "seen", "payload": [{ "name": "msg", "type": "text" }] });
        let em = g.add_node(NodeKind::EmitEvent, 0, 0);
        g.node_mut(em).unwrap().config = schema.clone();
        let on = g.add_node(NodeKind::OnEvent, 0, 0);
        g.node_mut(on).unwrap().config = schema;

        // EmitEvent gains a Text INPUT port "msg"; OnEvent a Text OUTPUT port "msg".
        assert_eq!(g.node(em).unwrap().input_port("msg").unwrap().ty, PortType::Text);
        assert_eq!(g.node(on).unwrap().output_port("msg").unwrap().ty, PortType::Text);

        // The dynamic ports wire by type: Format.text → EmitEvent.msg, OnEvent.msg → Format.p1.
        let src = g.add_node(NodeKind::Format, 0, 0);
        g.add_edge(src, "text", em, "msg").expect("payload INPUT port wires by type");
        let sink = g.add_node(NodeKind::Format, 0, 0);
        g.add_edge(on, "exec_out", sink, "exec_in").unwrap();
        g.add_edge(on, "msg", sink, "p1").expect("payload OUTPUT port wires by type");

        // Firing the handler with a payload seeds the OnEvent's "msg" output for downstream.
        let entry = on_event_node(&g, "seen").unwrap();
        let mut payload = PortValues::new();
        payload.insert("msg".into(), PortValue::Text("hello".into()));
        let outs = run_subpath_eventful(&g, &ctx, entry, payload, &mut |_| {}, &mut |_, _, _| {}, &mut |_, _| {}).unwrap();
        assert_eq!(
            outs.get(&on).and_then(|m| m.get("msg")).and_then(PortValue::as_text),
            Some("hello"),
            "OnEvent exposes the delivered payload on its named output"
        );
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
    fn ask_ai_routes_true_branch_on_yes_answer() {
        let ctx = ctx_with_vision(true);
        let mut g = Graph::new(1, "g");
        let ai = g.add_node(NodeKind::AskAi, 0, 0);
        g.node_mut(ai).unwrap().config = json!({ "question": "Is someone there?", "timeout_secs": "65" });
        let yes = g.add_node(NodeKind::Format, 0, 0);
        let no = g.add_node(NodeKind::Format, 0, 0);
        g.add_edge(ai, "true", yes, "exec_in").unwrap();
        g.add_edge(ai, "false", no, "exec_in").unwrap();
        let outs = run_dag(&g, &ctx).unwrap();
        assert!(outs.contains_key(&ai), "AI If ran");
        assert!(outs.contains_key(&yes), "'true' branch ran (answer = YES)");
        assert!(!outs.contains_key(&no), "'false' branch skipped");
    }

    #[test]
    fn ask_ai_routes_false_branch_on_no_answer() {
        let ctx = ctx_with_vision(false);
        let mut g = Graph::new(1, "g");
        let ai = g.add_node(NodeKind::AskAi, 0, 0);
        g.node_mut(ai).unwrap().config = json!({ "question": "Is someone there?", "timeout_secs": "65" });
        let yes = g.add_node(NodeKind::Format, 0, 0);
        let no = g.add_node(NodeKind::Format, 0, 0);
        g.add_edge(ai, "true", yes, "exec_in").unwrap();
        g.add_edge(ai, "false", no, "exec_in").unwrap();
        let outs = run_dag(&g, &ctx).unwrap();
        assert!(!outs.contains_key(&yes), "'true' branch skipped");
        assert!(outs.contains_key(&no), "'false' branch ran (answer = NO)");
    }

    #[test]
    fn ask_ai_errors_without_question() {
        let ctx = ctx_with_vision(false);
        let mut g = Graph::new(1, "g");
        let ai = g.add_node(NodeKind::AskAi, 0, 0);
        // No question in config or wired
        g.node_mut(ai).unwrap().config = json!({ "question": "" });
        g.run_orphans = true;
        let err = run_dag(&g, &ctx).unwrap_err();
        assert!(err.contains("question is required"), "got: {err}");
    }

    #[test]
    fn ask_ai_wires_true_false_exec_ports() {
        let mut g = Graph::new(1, "g");
        let ai = g.add_node(NodeKind::AskAi, 0, 0);
        let t = g.add_node(NodeKind::Format, 0, 0);
        let f = g.add_node(NodeKind::Format, 0, 0);
        g.add_edge(ai, "true", t, "exec_in").expect("true port wires");
        g.add_edge(ai, "false", f, "exec_in").expect("false port wires");
        // image and question wire correctly
        let snap = g.add_node(NodeKind::CameraSnapshot, 0, 0);
        g.add_edge(snap, "image", ai, "image").expect("image wires");
    }

    #[test]
    fn string_switch_routes_to_matching_arm() {
        // StringSwitch("red") → fires the "red" arm; "blue" and "default" are skipped.
        let ctx = test_ctx();
        let mut g = Graph::new(1, "g");
        let sw = g.add_node(NodeKind::StringSwitch, 0, 0);
        g.node_mut(sw).unwrap().config = json!({ "arms": ["red", "blue", "green"], "value": "red" });
        let red = g.add_node(NodeKind::Format, 0, 0);
        let blue = g.add_node(NodeKind::Format, 0, 0);
        let def = g.add_node(NodeKind::Format, 0, 0);
        g.add_edge(sw, "red", red, "exec_in").unwrap();
        g.add_edge(sw, "blue", blue, "exec_in").unwrap();
        g.add_edge(sw, "default", def, "exec_in").unwrap();
        let outs = run_dag(&g, &ctx).unwrap();
        assert!(outs.contains_key(&sw), "switch ran");
        assert!(outs.contains_key(&red), "matching arm 'red' ran");
        assert!(!outs.contains_key(&blue), "'blue' skipped");
        assert!(!outs.contains_key(&def), "'default' skipped when a match fires");
    }

    #[test]
    fn string_switch_fires_default_when_no_arm_matches() {
        // StringSwitch("purple") → no arm matches → "default" fires, named arms skip.
        let ctx = test_ctx();
        let mut g = Graph::new(1, "g");
        let sw = g.add_node(NodeKind::StringSwitch, 0, 0);
        g.node_mut(sw).unwrap().config = json!({ "arms": ["red", "blue"], "value": "purple" });
        let red = g.add_node(NodeKind::Format, 0, 0);
        let def = g.add_node(NodeKind::Format, 0, 0);
        g.add_edge(sw, "red", red, "exec_in").unwrap();
        g.add_edge(sw, "default", def, "exec_in").unwrap();
        let outs = run_dag(&g, &ctx).unwrap();
        assert!(!outs.contains_key(&red), "'red' skipped");
        assert!(outs.contains_key(&def), "'default' fires on no match");
    }

    #[test]
    fn string_switch_value_from_upstream_node() {
        // Wired value: Format → "blue" → StringSwitch routes to blue arm.
        let ctx = test_ctx();
        let mut g = Graph::new(1, "g");
        let fmt = g.add_node(NodeKind::Format, 0, 0);
        g.node_mut(fmt).unwrap().config = json!({ "template": "blue", "p1": "", "p2": "", "p3": "" });
        let sw = g.add_node(NodeKind::StringSwitch, 0, 0);
        g.node_mut(sw).unwrap().config = json!({ "arms": ["red", "blue", "green"] });
        g.add_edge(fmt, "exec_out", sw, "exec_in").unwrap();
        g.add_edge(fmt, "text", sw, "value").unwrap();
        let blue = g.add_node(NodeKind::Format, 0, 0);
        let def = g.add_node(NodeKind::Format, 0, 0);
        g.add_edge(sw, "blue", blue, "exec_in").unwrap();
        g.add_edge(sw, "default", def, "exec_in").unwrap();
        let outs = run_dag(&g, &ctx).unwrap();
        assert!(outs.contains_key(&blue), "'blue' arm fires (wired value = \"blue\")");
        assert!(!outs.contains_key(&def), "'default' skipped");
    }

    #[test]
    fn string_switch_missing_value_errors() {
        // No wired value and no config literal → run errors on missing required input.
        let ctx = test_ctx();
        let mut g = Graph::new(1, "g");
        let sw = g.add_node(NodeKind::StringSwitch, 0, 0);
        // arms set but no value literal or wire
        g.node_mut(sw).unwrap().config = json!({ "arms": ["red"] });
        g.run_orphans = true;
        let err = run_dag(&g, &ctx).unwrap_err();
        assert!(err.contains("missing required input 'value'"), "got: {err}");
    }

    // ---- CameraSwitch routing -----------------------------------------------
    // Helper: build a ToolContext pre-populated with two test cameras.
    // cam_a: ONVIF id "urn:uuid:aaa", ip "10.0.0.1", name "hall"
    // cam_b: no ONVIF id (legacy), ip "10.0.0.2", name "lobby"
    fn test_ctx_with_cameras() -> ToolContext {
        use crate::devices::Device;
        let cam_a = Device { id: "urn:uuid:aaa".into(), ip: "10.0.0.1".into(), xaddr: "".into(), name: "hall".into(), manufacturer: None, model: None, username: None, password: None, rtsp_url: None, rtsp_sub_url: None };
        let cam_b = Device { id: "".into(), ip: "10.0.0.2".into(), xaddr: "".into(), name: "lobby".into(), manufacturer: None, model: None, username: None, password: None, rtsp_url: None, rtsp_sub_url: None };
        let (tx, _rx) = std::sync::mpsc::channel();
        ToolContext::new_local(
            std::sync::Arc::new(std::sync::Mutex::new(vec![cam_a, cam_b])),
            crate::db::Db::lazy("postgres://x@localhost/x"),
            crate::tenancy::DEFAULT_TENANT_ID,
            std::sync::Arc::new(std::sync::Mutex::new(vec![])),
            std::sync::Arc::new(std::sync::Mutex::new(vec![])),
            None,
            tx,
        )
    }

    // Build a graph with a CameraSwitch node that gets the camera from a literal in config.
    // The engine resolves `config["camera"]` as a camera entity (via resolve_camera_literal).
    fn make_camera_switch_graph(camera_name: &str, arms: serde_json::Value) -> (Graph, u32, u32, u32, u32) {
        let mut g = Graph::new(1, "g");
        g.run_orphans = true; // run the switch even without an upstream exec trigger
        let sw = g.add_node(NodeKind::CameraSwitch, 0, 0);
        g.node_mut(sw).unwrap().config = serde_json::json!({ "arms": arms, "camera": camera_name });
        let arm1 = g.add_node(NodeKind::Format, 0, 0);
        let arm2 = g.add_node(NodeKind::Format, 0, 0);
        let def = g.add_node(NodeKind::Format, 0, 0);
        g.add_edge(sw, "arm_1", arm1, "exec_in").unwrap();
        g.add_edge(sw, "arm_2", arm2, "exec_in").unwrap();
        g.add_edge(sw, "default", def, "exec_in").unwrap();
        (g, sw, arm1, arm2, def)
    }

    #[test]
    fn camera_switch_routes_by_camera_id() {
        // Camera "hall" (ONVIF id "urn:uuid:aaa"): arm_1 stores that id → arm_1 fires.
        let ctx = test_ctx_with_cameras();
        let (g, sw, arm1, arm2, def) = make_camera_switch_graph(
            "hall", json!(["urn:uuid:aaa", "10.0.0.2"]),
        );
        let outs = run_dag(&g, &ctx).unwrap();
        assert!(outs.contains_key(&sw), "switch ran");
        assert!(outs.contains_key(&arm1), "arm_1 (hall by id) fires");
        assert!(!outs.contains_key(&arm2), "arm_2 skipped");
        assert!(!outs.contains_key(&def), "default skipped when match found");
    }

    #[test]
    fn camera_switch_routes_by_camera_ip_legacy() {
        // Legacy camera "lobby" (no ONVIF id, ip "10.0.0.2"): arm_2 stores the IP → fires.
        let ctx = test_ctx_with_cameras();
        let (g, _sw, arm1, arm2, def) = make_camera_switch_graph(
            "lobby", json!(["urn:uuid:aaa", "10.0.0.2"]),
        );
        let outs = run_dag(&g, &ctx).unwrap();
        assert!(!outs.contains_key(&arm1), "arm_1 skipped");
        assert!(outs.contains_key(&arm2), "arm_2 (lobby by ip) fires");
        assert!(!outs.contains_key(&def), "default skipped");
    }

    #[test]
    fn camera_switch_fires_default_when_no_arm_matches() {
        // Camera ip/id not in any arm → default fires; named arm skips.
        let ctx = test_ctx_with_cameras();
        let mut g = Graph::new(1, "g");
        g.run_orphans = true;
        let sw = g.add_node(NodeKind::CameraSwitch, 0, 0);
        g.node_mut(sw).unwrap().config = json!({ "arms": ["10.0.0.99"], "camera": "hall" });
        let arm1 = g.add_node(NodeKind::Format, 0, 0);
        let def = g.add_node(NodeKind::Format, 0, 0);
        g.add_edge(sw, "arm_1", arm1, "exec_in").unwrap();
        g.add_edge(sw, "default", def, "exec_in").unwrap();
        let outs = run_dag(&g, &ctx).unwrap();
        assert!(!outs.contains_key(&arm1), "arm_1 skipped (no match)");
        assert!(outs.contains_key(&def), "default fires when nothing matches");
    }

    #[test]
    fn camera_switch_empty_arm_string_never_matches() {
        // Empty string arms (old default config ["", ""]) must never match any camera —
        // the bug: `"" == cam.id` was true for legacy cameras with no ONVIF id.
        let ctx = test_ctx_with_cameras();
        let (g, _sw, arm1, _arm2, def) = make_camera_switch_graph(
            "lobby", json!(["", ""]), // lobby has id="" — must NOT false-match
        );
        let outs = run_dag(&g, &ctx).unwrap();
        assert!(!outs.contains_key(&arm1), "empty arm must not false-match a legacy camera (id=\"\")");
        assert!(outs.contains_key(&def), "default fires for unset arms");
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
