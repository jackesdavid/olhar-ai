//! Workflows engine — topological run + per-graph trigger/concurrency logic.
//! Ported from the ratatui `src/workflows/engine.rs`; only the `ToolContext` and
//! `Alert` import paths change (web core).
//!
//! `run_dag` executes a whole graph once, fire-and-forget: source nodes fetch
//! their own data, reachable nodes run in topological order (each at most once,
//! resolving inputs from upstream outputs else config literals), stopping on the
//! first node error. The trigger/concurrency methods are the pure decision logic
//! the backend poller drives.
use super::domain::{
    camera_switch_arms, string_switch_arms, ConcurrencyPolicy, Graph, NodeKind, PortType, EXEC_IN,
    WINDOW_STOP_MARKER,
};
use super::node::{self, PortValue, PortValues};
use super::repo;
use crate::agent::tools::{ToolContext, ToolUiEvent};

use base64::Engine as _;
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
        if let Ok(serde_json::Value::Array(arr)) =
            serde_json::from_str::<serde_json::Value>(trimmed)
        {
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
    run_driver(
        graph,
        sys,
        true,
        &HashSet::new(),
        &PortValues::new(),
        on_node,
        on_log,
        &mut |_, _| {},
    )
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
    run_driver(
        graph,
        sys,
        true,
        &HashSet::new(),
        &PortValues::new(),
        on_node,
        on_log,
        emit,
    )
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
    run_driver(
        graph,
        sys,
        false,
        &forced,
        &entry_payload,
        on_node,
        on_log,
        emit,
    )
}

/// Find the `OnEvent` node subscribing to `event`, if any (first match in node order) — GATED by
/// camera when both the node and the event have one: `event_camera` is the firing event's own
/// camera (`payload["camera"]`, `None` for events with no camera concept, e.g. `telegram_message`);
/// `devices` resolves a node's `camera` input (an id/ip/name selector, wired from
/// `GetCameraByName` or a plain config literal — see `Graph::resolve_camera_selector`) to compare
/// against it. A node with no camera scoped (the common case today) matches any camera, same as
/// before this existed. This is the general "event inputs gate the trigger" mechanism — the place
/// to extend for the next input that should filter delivery, not a new node kind.
pub fn on_event_node(
    graph: &Graph,
    event: &str,
    event_camera: Option<&str>,
    devices: &[crate::devices::Device],
) -> Option<u32> {
    graph
        .nodes
        .iter()
        .find(|n| {
            if n.kind != NodeKind::OnEvent
                || n.config.get("event").and_then(|v| v.as_str()) != Some(event)
            {
                return false;
            }
            let Some(want_ip) = event_camera else {
                return true;
            }; // event has no camera concept
            match graph.resolve_camera_selector(n) {
                None => true, // node isn't scoped to a camera — matches any
                Some(sel) => crate::device_actions::resolve_device(devices, &sel)
                    .map(|idx| devices[idx].ip == want_ip)
                    .unwrap_or(false),
            }
        })
        .map(|n| n.id)
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

    // A window-stop run RESUMES its instance's VARIABLES. `sys.variables` is a fresh per-run map —
    // the host seeds it with the graph's declared defaults — so without this a `Get` on the stop
    // branch reads a type-zero for anything a `Set` wrote on the arming run, and StopRecording's
    // required `camera`/`clip_id` inputs resolve to nothing → the branch is skipped as dead, the run
    // reporting ok (#114). Restore the arming run's snapshot ON TOP of the defaults (snapshot wins),
    // ONCE, before the epochs — a Set on the stop branch then still overrides it. Read from the
    // DelayTrigger that owns THIS trailing edge (the forced entry) and this instance, so one
    // camera's variables can't surface in another's stop. Done here rather than per-epoch so it
    // can't clobber a Set that ran in an earlier epoch.
    if entry_payload.contains_key(WINDOW_STOP_MARKER) {
        let instance_key = graph.instance_scope.key_for(entry_payload);
        for nid in initial_forced {
            if graph.node(*nid).map(|n| n.kind) != Some(NodeKind::DelayTrigger) {
                continue;
            }
            let Some(stored) =
                repo::get_node_variables(&sys.db, sys.tenant_id, graph.id, *nid, &instance_key)
            else {
                continue;
            };
            let vars = crate::workflows::values::variables_from_json(&stored);
            let mut map = sys.variables.lock().unwrap();
            for (k, v) in vars {
                map.insert(k, v);
            }
        }
    }

    let mut outputs: HashMap<u32, PortValues> = HashMap::new();
    let mut ran: HashSet<u32> = HashSet::new();
    let mut steps: u32 = 0;
    // Per-run iteration state for `ForEach` nodes (node id → next index), persisted across
    // epochs so each epoch advances the loop one element.
    let mut loop_state: HashMap<u32, usize> = HashMap::new();
    // Deferred `GetVariable` log/UI emits (#115). A pure `GetVariable` activates on EVERY run/epoch
    // (load-bearing for sub-runs — see the activation block in `forward_epoch`), so it fires even
    // when the only nodes it feeds sit on an unreached exec branch, producing a misleading log line
    // and canvas highlight. We can't decide at Get-run time whether its consumer will run (the Get
    // runs producer-before-consumer), so `forward_epoch` buffers the Get's `on_log` here (once per
    // node id, in activation order) and we flush below, after every epoch, keeping only those whose
    // consumer actually ran. Activation/value semantics are untouched, so #114 can't regress.
    let mut deferred_get_logs: Vec<(u32, String, u128)> = Vec::new();
    let mut reentries = forward_epoch(
        graph,
        sys,
        &indeg_tmpl,
        &adj,
        &back,
        &connected,
        seed_entries,
        initial_forced,
        entry_payload,
        &mut outputs,
        &mut ran,
        &mut loop_state,
        &mut steps,
        on_node,
        on_log,
        emit,
        &mut deferred_get_logs,
    )?;
    // Later epochs: re-enter loops at the back-edge / ForEach targets until none fire.
    while !reentries.is_empty() {
        let forced: HashSet<u32> = reentries.into_iter().collect();
        reentries = forward_epoch(
            graph,
            sys,
            &indeg_tmpl,
            &adj,
            &back,
            &connected,
            false,
            &forced,
            entry_payload,
            &mut outputs,
            &mut ran,
            &mut loop_state,
            &mut steps,
            on_node,
            on_log,
            emit,
            &mut deferred_get_logs,
        )?;
    }

    // Flush the deferred `GetVariable` logs (#115). Emit a Get's log line + canvas highlight only
    // when ≥1 node it feeds (a data edge `from == get`) is in `ran` this run. A Get whose consumers
    // all sat out on the other branch — or with no consumers at all — stays quiet. Emitted in
    // activation order, once per Get (the buffer is already deduped), so a normal single-branch
    // graph is unaffected (its Gets all feed ran nodes → all still logged).
    for (nid, summary, elapsed) in &deferred_get_logs {
        if graph
            .edges
            .iter()
            .any(|e| e.from == *nid && ran.contains(&e.to))
        {
            on_log(*nid, summary, *elapsed);
        }
    }

    // Record what this run produced, so a later run of the SAME instance can read it — which is the
    // only way a `DelayTrigger`'s trailing edge can ever see `camera` or `clip_id`, since it runs
    // from a timer with no payload of its own. It's also the store the live instance view reads
    // (#94 phase 2), so this now runs for EVERY graph — the guard that once limited it to graphs
    // with a latent node is gone: the codec drops media (no base64 in a row), so a per-node write
    // per run is cheap, and the editor's live values need the same rows a DelayTrigger does.
    //
    // Only nodes that actually RAN are written. That's what makes a re-trigger correct: it doesn't
    // re-run `StartRecording` (only `start` lights it, and a re-trigger lights nothing), so the
    // arming run's `clip_id` survives untouched while the camera refreshes. Write everything in
    // `outputs` instead and a stop run would immediately overwrite the values it had just resumed.
    let instance_key = graph.instance_scope.key_for(entry_payload);
    for nid in &ran {
        let Some(produced) = outputs.get(nid) else {
            continue;
        };
        if produced.is_empty() {
            continue;
        }
        let mut one = crate::workflows::values::NodeValues::new();
        one.insert(*nid, produced.clone());
        let json = crate::workflows::values::to_json(&one);
        repo::set_node_values(&sys.db, sys.tenant_id, graph.id, *nid, &instance_key, &json);
    }

    // Variables snapshot stays DelayTrigger-only: it's the arming run's `sys.variables` restored on
    // the trailing edge (#114), meaningless to a graph with no latent window.
    if graph.nodes.iter().any(|n| n.kind == NodeKind::DelayTrigger) {
        // Variables are a run-scoped map (`sys.variables`), not a node output — 0058's `values_json`
        // doesn't carry them, so a `Get current_camera` on the trailing edge read a type-zero for
        // whatever a `Set current_camera` wrote on the arming run (#114). Snapshot the whole map onto
        // the DelayTrigger's OWN row whenever it ran this run (armed or extended the window), keyed
        // by the same instance — the stop run restores it below. Gated to a DelayTrigger that
        // actually ran so an unrelated run of the graph can't overwrite a live window's snapshot.
        for n in graph
            .nodes
            .iter()
            .filter(|n| n.kind == NodeKind::DelayTrigger && ran.contains(&n.id))
        {
            let vars_json = {
                let map = sys.variables.lock().unwrap();
                crate::workflows::values::variables_to_json(&map)
            };
            repo::set_node_variables(
                &sys.db,
                sys.tenant_id,
                graph.id,
                n.id,
                &instance_key,
                &vars_json,
            );
        }
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
    deferred_get_logs: &mut Vec<(u32, String, u128)>,
) -> Result<Vec<u32>, String> {
    let mut indeg = indeg_tmpl.clone();
    let mut settled: HashSet<u32> = HashSet::new();
    // Which instance of the graph is this run? The GRAPH decides (Unreal's model: the graph is the
    // Blueprint, the instance is the actor) — no node ever learns what a camera is. `Single` yields
    // "", which is byte-for-byte the behaviour before instances existed, so every graph alive today
    // addresses the same state rows it always did.
    //
    // It scopes ALL latent node state, not just the DelayTrigger's window: a Cooldown or Time Gate
    // in a per-camera graph is per-camera too. Scoping only one node kind would be the fork this
    // model exists to remove.
    let instance_key = graph.instance_scope.key_for(entry_payload);

    // A window-stop run RESUMES its instance: it's fired by a timer and carries none of the event's
    // data, so without the instance's stored values `StopRecording.camera` (from OnEvent) and
    // `clip_id` (from StartRecording) resolve to nothing and the branch is skipped as dead — the
    // original bug. Seeding them is what "the run never really ended" means in practice.
    //
    // Only on stop runs. Seeding every run would make stale values look live and revive branches the
    // engine deliberately gated off.
    if entry_payload.contains_key(WINDOW_STOP_MARKER) {
        for n in &graph.nodes {
            if outputs.contains_key(&n.id) {
                continue;
            }
            let Some(stored) =
                repo::get_node_values(&sys.db, sys.tenant_id, graph.id, n.id, &instance_key)
            else {
                continue;
            };
            let vals = crate::workflows::values::from_json(&stored);
            if let Some(pv) = vals.get(&n.id) {
                outputs.insert(n.id, pv.clone());
            }
        }
    }

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
        } else if n.kind.is_pure_source() {
            // Pure sources re-evaluate every epoch (no exec propagation): a `GetVariable` consumer
            // sees the value as of when it runs — after any upstream `Set` — rather than a stale
            // epoch-0 read. Cheap; never extends the run.
            //
            // It also keeps them alive on SUB-RUNS. A source with no incoming edges is a true
            // orphan, and orphans below fall back to `seed_entries`, which is false for every
            // `OnEvent`/timer sub-path — so the source silently wouldn't run and its consumers'
            // required inputs would be skipped as a dead branch. See `NodeKind::is_pure_source`.
            true
        } else if has_ord_exec {
            fed_live
        } else {
            // No exec_in wired: pure data/logic nodes and true entry orphans. For nodes
            // with upstream data edges (comparison, conversion, etc.), activate as soon as
            // any upstream producer has run — they're pulled by the exec chain, not seeded.
            // True orphans (no incoming ordering edges at all) fall back to seed_entries.
            let has_upstream = graph
                .edges
                .iter()
                .any(|e| e.to == nid && !back.contains(&(e.from, nid)));
            if has_upstream {
                graph
                    .edges
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
            if let Some(edge) = graph
                .edges
                .iter()
                .find(|e| e.to == nid && e.to_port == port.name)
            {
                // A pure source (GetVariable/GetCameraByName/GetRecipient) reads AMBIENT state —
                // the run's variable map — not a wired input, so nothing in the graph orders it
                // against the `SetVariable` that fills that state. It therefore lands wherever the
                // topological tie-break puts it, which is by node id: a Get created before its Set
                // ran first and read an empty variable. For most types that surfaced as a silent
                // zero (an all-zero bbox); for Image/File/Person/Face/Camera, whose `zero_value` is
                // None, it surfaced as "missing required input".
                //
                // Re-evaluate it HERE, at the moment its consumer reads it, so it observes the
                // state the exec chain has actually produced by this point. Ordering then follows
                // the wires — a Set wired earlier in the flow is seen — instead of node ids.
                let pure_now = graph
                    .node(edge.from)
                    .filter(|u| u.kind.is_pure_source())
                    .and_then(|u| {
                        node::run_node(u.kind, &u.config, &PortValues::new(), sys)
                            .ok()
                            .and_then(|mut o| o.remove(&edge.from_port))
                    });
                if let Some(v) = pure_now {
                    inputs.insert(port.name.clone(), v);
                    continue;
                }
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
            } else if port.ty == PortType::Table {
                // Table literals resolve a dictionary id against the run-start snapshot (needs sys).
                if let Some(t) = node::resolve_table_literal(&n.config, &port.name, sys) {
                    inputs.insert(port.name.clone(), t);
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
                on_log(
                    nid,
                    "memoize on for a node on a loop — it won't iterate (turn memoize off to loop)",
                    0,
                );
            }
            settle(&mut settled, &mut indeg);
            continue;
        }

        if *steps >= WF_MAX_STEPS {
            return Err(format!(
                "workflow exceeded step budget ({WF_MAX_STEPS} steps) — possible infinite loop"
            ));
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
                None => {
                    parse_text_items(n.config.get("items").and_then(|v| v.as_str()).unwrap_or(""))
                }
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
            let state = repo::get_node_state(&sys.db, sys.tenant_id, graph.id, nid, &instance_key);
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
                &instance_key,
                &serde_json::json!({ "is_a": !is_a }),
            );
            live_out.insert((nid, if is_a { "a" } else { "b" }.to_string()));
            let mut o = PortValues::new();
            o.insert("is_a".into(), PortValue::Bool(is_a));
            *steps += 1;
            if graph.debug {
                on_log(
                    nid,
                    &format!("Flip Flop → {}", if is_a { "a" } else { "b" }),
                    0,
                );
            }
            outputs.insert(nid, o);
            ran.insert(nid);
            settle(&mut settled, &mut indeg);
            continue;
        }

        // Cooldown: pass exec through only when `window_secs` have elapsed since last fire.
        // Stores last-fired Unix-ms in node state; first run always passes.
        if n.kind == NodeKind::Cooldown {
            let window_secs = n
                .config
                .get("window_secs")
                .and_then(|v| {
                    v.as_u64()
                        .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
                })
                .unwrap_or(60);
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            let state = repo::get_node_state(&sys.db, sys.tenant_id, graph.id, nid, &instance_key);
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
                    &instance_key,
                    &serde_json::json!({ "last_ms": now_ms }),
                );
                live_out.insert((nid, "exec_out".to_string()));
            }
            let mut o = PortValues::new();
            o.insert("elapsed_secs".into(), PortValue::Num(elapsed_secs as i64));
            o.insert("ok".into(), PortValue::Bool(ok));
            *steps += 1;
            if graph.debug {
                on_log(
                    nid,
                    &format!(
                        "Cooldown: {elapsed_secs}s elapsed / {window_secs}s window → {}",
                        if ok { "pass" } else { "blocked" }
                    ),
                    0,
                );
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
            let wait_secs = n
                .config
                .get("wait_secs")
                .and_then(|v| {
                    v.as_i64()
                        .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
                })
                .unwrap_or(10);
            let extend_secs = n
                .config
                .get("extend_secs")
                .and_then(|v| {
                    v.as_i64()
                        .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
                })
                .unwrap_or(wait_secs);
            let now_ts = Utc::now().timestamp();
            let state = repo::get_node_state(&sys.db, sys.tenant_id, graph.id, nid, &instance_key);
            let is_armed = state
                .as_ref()
                .and_then(|v| v.get("state"))
                .and_then(|v| v.as_str())
                .map(|s| s == "armed")
                .unwrap_or(false);

            let gate_key = (sys.tenant_id, graph.id, nid);
            let keeper_alive = sys.active_gates.lock().unwrap().contains(&gate_key);

            if is_armed && keeper_alive {
                // Re-trigger: a keeper is running. Extend expires_at if needed, then return.
                let expires_ts = state
                    .as_ref()
                    .and_then(|v| v.get("expires_at"))
                    .and_then(|v| v.as_str())
                    .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                    .map(|t| t.timestamp())
                    .unwrap_or(now_ts + wait_secs);
                let remaining = expires_ts - now_ts;
                if remaining < extend_secs {
                    let new_expires_ts = now_ts + extend_secs;
                    repo::set_node_state(
                        &sys.db,
                        sys.tenant_id,
                        graph.id,
                        nid,
                        &instance_key,
                        &serde_json::json!({
                            "state": "armed",
                            "expires_at": chrono::DateTime::from_timestamp(new_expires_ts, 0)
                                .unwrap_or_else(Utc::now)
                                .to_rfc3339(),
                        }),
                    );
                    if graph.debug {
                        on_log(
                            nid,
                            &format!("Time Gate: extended +{}s", extend_secs - remaining),
                            0,
                        );
                    }
                } else if graph.debug {
                    on_log(
                        nid,
                        &format!("Time Gate: re-trigger, {remaining}s remaining (no extend)"),
                        0,
                    );
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
            repo::set_node_state(
                &sys.db,
                sys.tenant_id,
                graph.id,
                nid,
                &instance_key,
                &serde_json::json!({
                    "state": "armed",
                    "expires_at": chrono::DateTime::from_timestamp(expires_ts, 0)
                        .unwrap_or_else(Utc::now)
                        .to_rfc3339(),
                }),
            );
            sys.active_gates.lock().unwrap().insert(gate_key);
            if graph.debug {
                if is_armed {
                    on_log(
                        nid,
                        &format!("Time Gate: keeper recovered, re-armed for {wait_secs}s"),
                        0,
                    );
                } else {
                    on_log(nid, &format!("Time Gate: armed for {wait_secs}s"), 0);
                }
            }

            let fired = loop {
                std::thread::sleep(Duration::from_millis(250));
                let st = repo::get_node_state(&sys.db, sys.tenant_id, graph.id, nid, &instance_key);
                let armed = st
                    .as_ref()
                    .and_then(|v| v.get("state"))
                    .and_then(|v| v.as_str())
                    .map(|s| s == "armed")
                    .unwrap_or(false);
                if !armed {
                    break false; // reset by an external write
                }
                let exp = st
                    .as_ref()
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
            repo::set_node_state(
                &sys.db,
                sys.tenant_id,
                graph.id,
                nid,
                &instance_key,
                &serde_json::json!({ "state": "idle" }),
            );
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

        // Delay Trigger: leading + trailing debounce. Two paths:
        //
        //  * Trigger (fed by a live exec pin) — try to arm. Winning the CAS means this is the
        //    first signal of an idle window, so fire `start` HERE, in this run, and Start
        //    Recording reaches the edge immediately. Losing means the window is already open,
        //    so the signal only extends it.
        //  * Expiry (the window consumer forced this node after the timer came due and it won
        //    the disarm) — fire `stop`, and the node is already back to idle, ready to re-arm.
        //
        // Unlike Time Gate this never blocks. A node's handler runs to completion before the
        // pass settles it, so blocking here would hold `start`'s downstream too. The wait lives
        // in `work_jobs` instead — which also means it survives a pod restart.
        if n.kind == NodeKind::DelayTrigger {
            use chrono::Utc;
            let cfg_secs = |key: &str, fallback: i64| -> i64 {
                n.config
                    .get(key)
                    .and_then(|v| {
                        v.as_i64()
                            .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
                    })
                    .unwrap_or(fallback)
            };
            let wait_secs = cfg_secs("wait_secs", 10);
            let extend_secs = cfg_secs("extend_secs", wait_secs);

            // Trailing edge. The consumer already won the disarm, so this run owns the stop.
            // The payload marker — not `forced` alone — is the test: a loop back-edge forces a
            // node too, and that must not be mistaken for an expired window.
            if forced.contains(&nid) && entry_payload.contains_key(WINDOW_STOP_MARKER) {
                live_out.insert((nid, "stop".to_string()));
                if graph.debug {
                    on_log(nid, "Delay Trigger: window closed → stop", 0);
                }
                *steps += 1;
                outputs.insert(nid, PortValues::new());
                ran.insert(nid);
                settle(&mut settled, &mut indeg);
                continue;
            }

            let now_ts = Utc::now().timestamp();
            let rfc = |ts: i64| {
                chrono::DateTime::from_timestamp(ts, 0)
                    .unwrap_or_else(Utc::now)
                    .to_rfc3339()
            };
            let expires_at = rfc(now_ts + wait_secs);

            if repo::try_arm_node_state(
                &sys.db,
                sys.tenant_id,
                graph.id,
                nid,
                &instance_key,
                &expires_at,
            ) {
                // Leading edge: this signal opened the window, so it owns the start.
                live_out.insert((nid, "start".to_string()));
                crate::work_queue::enqueue_window_stop_blocking(
                    &sys.db,
                    sys.tenant_id,
                    graph.id,
                    nid,
                    &instance_key,
                    &expires_at,
                );
                if graph.debug {
                    on_log(
                        nid,
                        &format!("Delay Trigger: armed for {wait_secs}s → start"),
                        0,
                    );
                }
            } else {
                // Already armed: a re-trigger tops the window up, and only when that actually
                // pushes the deadline out (Time Gate's rule: remaining = max(remaining, extend)).
                let state =
                    repo::get_node_state(&sys.db, sys.tenant_id, graph.id, nid, &instance_key);
                let expires_ts = state
                    .as_ref()
                    .and_then(|v| v.get("expires_at"))
                    .and_then(|v| v.as_str())
                    .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                    .map(|t| t.timestamp())
                    .unwrap_or(now_ts);
                let remaining = expires_ts - now_ts;
                if remaining < extend_secs {
                    repo::extend_node_state(
                        &sys.db,
                        sys.tenant_id,
                        graph.id,
                        nid,
                        &instance_key,
                        &rfc(now_ts + extend_secs),
                    );
                    if graph.debug {
                        on_log(
                            nid,
                            &format!("Delay Trigger: extended +{}s", extend_secs - remaining),
                            0,
                        );
                    }
                } else if graph.debug {
                    on_log(
                        nid,
                        &format!("Delay Trigger: re-trigger, {remaining}s remaining (no extend)"),
                        0,
                    );
                }
                // The scheduled stop still carries the old expiry — the consumer re-reads the
                // state when it comes due and reschedules itself if the window moved.
            }
            *steps += 1;
            outputs.insert(nid, PortValues::new());
            ran.insert(nid);
            settle(&mut settled, &mut indeg);
            continue;
        }

        // Device Gate: ask a person, on their phone, before continuing. Two paths, and the node
        // takes NO arm on the way in — the run ends at the ask.
        //
        //  * Resume (the answer endpoint or the expiry sweep forced this node, and the payload
        //    carries a verdict) — fire the arm the verdict names.
        //  * Ask (an ordinary exec pin) — mint the request, push it, and stop. The run ends here.
        //    Not because asking is slow, but because a run cannot block on a human: a handler runs
        //    to completion before the pass settles it, so waiting here would hold the whole graph
        //    for as long as someone leaves their phone face-down.
        if n.kind == NodeKind::DeviceGate {
            // Resume. The PRESENCE of the verdict key is the test, not `forced` alone — a loop
            // back-edge forces a node too, and firing an arm nobody chose would be worse than not
            // resuming at all.
            if forced.contains(&nid) {
                if let Some(verdict) = entry_payload
                    .get(crate::workflows::domain::DEVICE_GATE_VERDICT)
                    .and_then(PortValue::as_text)
                {
                    let arm = super::device_gate::GateState::from_str(verdict)
                        .unwrap_or(super::device_gate::GateState::Failed)
                        .exec_arm();
                    live_out.insert((nid, arm.to_string()));

                    let mut o = PortValues::new();
                    let carried = |k: &str| {
                        entry_payload
                            .get(k)
                            .and_then(PortValue::as_text)
                            .unwrap_or_default()
                            .to_string()
                    };
                    o.insert(
                        "answered_by".into(),
                        PortValue::Text(carried(
                            crate::workflows::domain::DEVICE_GATE_ANSWERED_BY,
                        )),
                    );
                    o.insert(
                        "factor".into(),
                        PortValue::Text(carried(crate::workflows::domain::DEVICE_GATE_FACTOR)),
                    );
                    on_log(nid, &format!("Device Gate: {verdict} → {arm}"), 0);
                    *steps += 1;
                    outputs.insert(nid, o);
                    ran.insert(nid);
                    settle(&mut settled, &mut indeg);
                    continue;
                }
            }

            // Ask. Both values may be wired or authored in the panel; `literal_for` is what makes
            // a `required` port satisfiable from config.
            let cfg_text = |key: &str| -> String {
                inputs
                    .get(key)
                    .and_then(PortValue::as_text)
                    .map(str::to_string)
                    .or_else(|| {
                        n.config
                            .get(key)
                            .and_then(|v| v.as_str())
                            .map(str::to_string)
                    })
                    .unwrap_or_default()
            };
            let recipient = cfg_text("recipient");
            let message = cfg_text("message");
            let ttl_secs = n
                .config
                .get("ttl_secs")
                .and_then(|v| {
                    v.as_i64()
                        .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
                })
                .unwrap_or(600);

            // An unset Recipient or an empty prompt is an AUTHORING mistake, and it fails the run
            // rather than taking `failure`. `failure` means a human was asked and did not decide;
            // letting a broken gate arrive on the same arm is how a graph comes to treat "never
            // asked" as "asked and ignored".
            if recipient.is_empty() {
                return Err("Device Gate: no Recipient set — nobody to ask".into());
            }
            if message.trim().is_empty() {
                return Err("Device Gate: the message is empty — nothing to decide on".into());
            }

            match super::device_gate::resolve_device_by_label(&sys.db, sys.tenant_id, &recipient) {
                Ok((device_id, token)) => {
                    // The cap is read per ask rather than cached: it is one indexed row, and a
                    // tenant that lowers it while a loop is running expects the next ask to obey.
                    let cap = super::device_gate::max_pending_for_tenant(&sys.db, sys.tenant_id);
                    match super::device_gate::mint(
                        &sys.db,
                        sys.tenant_id,
                        graph.id,
                        nid,
                        sys.run_id,
                        device_id,
                        &message,
                        &graph.name,
                        ttl_secs,
                        cap,
                    ) {
                        Ok(minted) => {
                            // The push carries the request id and the raw token — the only copy
                            // that will ever exist. The body carries the message and the flow name
                            // so the decision is legible from the lock screen.
                            // The token rides in `data`, which the app reads and the OS never
                            // renders. It must never reach `body` — that would print a
                            // gate-opening secret on the lock screen.
                            let payload = serde_json::json!({
                                "kind": "device_gate",
                                "request_id": minted.id,
                                "token": minted.token,
                                "expires_at": minted.expires_at,
                            });
                            // The flow name is the title, so "which automation wants this?" is
                            // answerable before unlocking — and it comes from the graph, never
                            // from text the prompt's author could make disagree with it.
                            let title = format!("{}: approval needed", graph.name);
                            match crate::notification::push::send_data(
                                &[token],
                                &title,
                                &message,
                                payload,
                            ) {
                                Ok(_) => {
                                    on_log(
                                        nid,
                                        &format!(
                                            "Device Gate: asked {recipient}, expires {}",
                                            minted.expires_at
                                        ),
                                        0,
                                    );
                                }
                                Err(e) => {
                                    // Undeliverable. Settle it now rather than leaving a request
                                    // nobody will ever see waiting for its TTL: the graph learns
                                    // in seconds instead of ten minutes, and the audit trail
                                    // records `failed` rather than `expired` — different events.
                                    super::device_gate::settle_failed(&sys.db, minted.id);
                                    live_out.insert((nid, "failure".to_string()));
                                    on_log(nid, &format!("Device Gate: push failed — {e}"), 0);
                                }
                            }
                        }
                        // At the cap. `failure` is right for the same reason expiry is: no human
                        // decided. The graph author sees the reason in the log rather than a run
                        // that took Failure for no visible cause — which is what a silent drop
                        // would look like from the canvas.
                        Err(super::device_gate::MintError::AtCap(cap)) => {
                            live_out.insert((nid, "failure".to_string()));
                            on_log(
                                nid,
                                &format!(
                                    "Device Gate: '{recipient}' already has {cap} unanswered \
                                     requests — not asking again until some are answered or expire"
                                ),
                                0,
                            );
                        }
                        Err(super::device_gate::MintError::Db) => {
                            live_out.insert((nid, "failure".to_string()));
                            on_log(nid, "Device Gate: could not create the approval request", 0);
                        }
                    }
                }
                Err(super::device_gate::ResolveError::NoDevice) => {
                    live_out.insert((nid, "failure".to_string()));
                    on_log(
                        nid,
                        &format!("Device Gate: '{recipient}' has no registered phone"),
                        0,
                    );
                }
                Err(super::device_gate::ResolveError::Ambiguous(count)) => {
                    live_out.insert((nid, "failure".to_string()));
                    on_log(
                        nid,
                        &format!(
                            "Device Gate: '{recipient}' matches {count} phones — only the addressed \
                             device may answer, so there is no one device to ask"
                        ),
                        0,
                    );
                }
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
            let summary = node::debug_summary(n.kind, &n.config, &inputs, &o);
            // A pure `GetVariable` activates every run/epoch even when the branch it feeds is never
            // reached (#115). Defer its emit rather than logging here — `run_driver` flushes it only
            // if a node it feeds actually ran. Buffer once per node id, in activation order; other
            // kinds log inline as before. (Only `GetVariable`: the reported, conservative case —
            // `GetCameraByName`/`GetRecipient` are left untouched.)
            if n.kind == NodeKind::GetVariable {
                if !deferred_get_logs.iter().any(|(id, _, _)| *id == nid) {
                    deferred_get_logs.push((nid, summary, elapsed_ms));
                }
            } else {
                on_log(nid, &summary, elapsed_ms);
            }
        }

        if n.kind == NodeKind::ShowImage {
            if let Some(jpeg) = inputs
                .get("image")
                .and_then(PortValue::as_image)
                .and_then(|b| node::to_jpeg(b).ok())
            {
                let label = n
                    .config
                    .get("label")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                // Persist the frame so it survives browser refresh and flow restarts.
                repo::set_node_state(
                    &sys.db,
                    sys.tenant_id,
                    graph.id,
                    nid,
                    &instance_key,
                    &serde_json::json!({
                        "jpeg_b64": base64::engine::general_purpose::STANDARD.encode(&jpeg),
                        "label": &label,
                    }),
                );
                let _ = sys.ui_tx.send(ToolUiEvent::ShowImage {
                    graph: graph.id,
                    node: nid,
                    label,
                    jpeg,
                });
            }
        }

        // Display live sinks (Text/Gauge): resolve the `value` input (wired, or the config literal
        // fallback), persist it (survives refresh / flow restart), and push it live to the editor.
        if matches!(n.kind, NodeKind::Text | NodeKind::Gauge) {
            let value = match inputs.get("value") {
                Some(PortValue::Text(s)) => serde_json::json!(s),
                Some(PortValue::Num(v)) => serde_json::json!(v),
                Some(PortValue::Bool(b)) => serde_json::json!(b),
                _ => serde_json::Value::Null,
            };
            if !value.is_null() {
                repo::set_node_state(
                    &sys.db,
                    sys.tenant_id,
                    graph.id,
                    nid,
                    &instance_key,
                    &serde_json::json!({ "value": &value }),
                );
                let _ = sys.ui_tx.send(ToolUiEvent::ShowValue {
                    graph: graph.id,
                    node: nid,
                    kind: n.kind.slug().to_string(),
                    value,
                });
            }
        }

        // EmitEvent: fire the event so the host spawns the (guarded) handler sub-run.
        // The node's gathered inputs ARE the event's typed payload (one per field).
        if n.kind == NodeKind::EmitEvent {
            if let Some(ev) = n
                .config
                .get("event")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
            {
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
            let cond = inputs
                .get("condition")
                .and_then(PortValue::as_bool)
                .unwrap_or(false);
            live_out.insert((nid, if cond { "true" } else { "false" }.to_string()));
        } else if n.kind == NodeKind::AskAi {
            // AskAi: run_node computed the answer and stored it in `o["result"]`.
            let answer = o
                .get("result")
                .and_then(PortValue::as_bool)
                .unwrap_or(false);
            live_out.insert((nid, if answer { "true" } else { "false" }.to_string()));
        } else if n.kind == NodeKind::StringSwitch {
            let val = inputs
                .get("value")
                .and_then(PortValue::as_text)
                .unwrap_or_default();
            let arms = string_switch_arms(&n.config);
            if arms.iter().any(|a| a == val) {
                live_out.insert((nid, val.to_string()));
            } else {
                live_out.insert((nid, "default".to_string()));
            }
        } else if n.kind == NodeKind::AiSwitch {
            // run_node classified the message and stored the chosen arm in o["arm"]. Validate it
            // against the configured arms (defence) and fire it, else the "default" branch.
            let arm = o
                .get("arm")
                .and_then(PortValue::as_text)
                .unwrap_or("default");
            let arms = string_switch_arms(&n.config);
            if arm != "default" && arms.iter().any(|a| a == arm) {
                live_out.insert((nid, arm.to_string()));
            } else {
                live_out.insert((nid, "default".to_string()));
            }
        } else if n.kind == NodeKind::CameraSwitch {
            let cam = inputs.get("camera").and_then(PortValue::as_camera);
            let port = if let Some(cam) = cam {
                let arms = camera_switch_arms(&n.config);
                let idx = arms
                    .iter()
                    .position(|a| !a.is_empty() && (a == &cam.ip || a == &cam.id));
                idx.map(|i| format!("arm_{}", i + 1))
                    .unwrap_or_else(|| "default".to_string())
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
    cron_last: HashMap<(uuid::Uuid, uuid::Uuid), Instant>,
    runs: HashMap<(uuid::Uuid, uuid::Uuid), GraphRun>,
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
    pub fn decide(
        &self,
        tenant: uuid::Uuid,
        graph_id: uuid::Uuid,
        policy: &ConcurrencyPolicy,
    ) -> Fire {
        let r = self
            .runs
            .get(&(tenant, graph_id))
            .copied()
            .unwrap_or_default();
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

    pub fn on_spawn(&mut self, tenant: uuid::Uuid, graph_id: uuid::Uuid) {
        self.runs.entry((tenant, graph_id)).or_default().running += 1;
    }

    pub fn on_queue(&mut self, tenant: uuid::Uuid, graph_id: uuid::Uuid) {
        self.runs.entry((tenant, graph_id)).or_default().queued += 1;
    }

    /// Call when a run finishes. Returns `true` if a queued fire should now spawn.
    pub fn on_finish(&mut self, tenant: uuid::Uuid, graph_id: uuid::Uuid) -> bool {
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

    // NOTE: no `total_running()` here on purpose. `runs` is per-graph policy accounting —
    // only `fire_graph` (cron/manual) feeds it via `on_spawn`, so a sum of it silently omits
    // every event handler. The pod-wide backstop counts the activity map instead
    // (`sentinel_flow::engine::total_in_flight`), which both spawn paths register with.

    /// Seed a graph's cron timer to a prior fire instant (Phase 0.5 restore). The adapter
    /// computes this anchor from the persisted wall-clock fire time so a mid-interval restart
    /// waits out the remainder. Pure: persistence lives in `workflows::repo`.
    pub fn seed_cron_last(&mut self, tenant: uuid::Uuid, graph_id: uuid::Uuid, last: Instant) {
        self.cron_last.insert((tenant, graph_id), last);
    }

    /// A tenant's current new-alert high-water mark (0 if unseen) — so the adapter can
    /// detect when a tick advanced it and persist the new value.
    pub fn alert_seen_max(&self, tenant: uuid::Uuid) -> u64 {
        self.alert_seen_max.get(&tenant).copied().unwrap_or(0)
    }

    /// Cron: returns `true` (and re-arms the timer) when `every_secs` has elapsed.
    pub fn cron_due(
        &mut self,
        tenant: uuid::Uuid,
        graph_id: uuid::Uuid,
        every_secs: u64,
        now: Instant,
    ) -> bool {
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
}

#[cfg(test)]
mod tests {
    use super::super::domain::{Graph, NodeKind, PortType};
    use super::*;
    use crate::agent::detector::{PersonDetection, PersonResult};
    use crate::ports::Vision;
    use serde_json::json;

    fn test_ctx() -> ToolContext {
        let (tx, _rx) = std::sync::mpsc::channel();
        ToolContext::new_local(
            std::sync::Arc::new(std::sync::Mutex::new(vec![])),
            crate::db::Db::lazy("postgres://sentinel:sentinel@localhost/sentinel_test"),
            crate::tenancy::DEFAULT_TENANT_ID,
            std::sync::Arc::new(std::sync::Mutex::new(vec![])),
            None,
            tx,
        )
    }

    /// A Vision stub whose `ask_yes_no` returns a fixed answer. Used to test `AskAi`
    /// routing without hitting the Anthropic API.
    struct StubVision(bool);
    impl Vision for StubVision {
        fn detect_persons(&self, _: &[u8]) -> Result<PersonResult, String> {
            Ok(PersonResult {
                person: false,
                count: 0,
                confidence: 0.0,
            })
        }
        fn detect_persons_detailed(&self, _: &[u8]) -> Result<Vec<PersonDetection>, String> {
            Ok(vec![])
        }
        fn detect_objects_detailed(
            &self,
            _: &[u8],
            _: u32,
        ) -> Result<Vec<PersonDetection>, String> {
            Ok(vec![])
        }
        fn describe(&self, _: &[u8], _: &str) -> Result<String, String> {
            Ok("stub".into())
        }
        fn ask_yes_no(&self, _: &str, _: Option<&[u8]>) -> Result<bool, String> {
            Ok(self.0)
        }
        fn ask_text(&self, _: &str, _: Option<&[u8]>) -> Result<String, String> {
            Ok("stub".into())
        }
        fn classify(&self, _: &str, _: &[String]) -> Result<Option<String>, String> {
            Ok(None)
        }
        fn extract(&self, _: &str, _: &str) -> Result<String, String> {
            Ok(String::new())
        }
    }

    fn ctx_with_vision(answer: bool) -> ToolContext {
        let (tx, _rx) = std::sync::mpsc::channel();
        let mut ctx = test_ctx();
        ctx.ui_tx = tx;
        ctx.vision = std::sync::Arc::new(StubVision(answer));
        ctx
    }

    /// The shape that silently died: a pure source with no incoming edges, feeding a required
    /// input, inside a sub-run.
    ///
    /// An orphan falls back to `seed_entries` — false for every `OnEvent`/timer sub-path — so the
    /// source never ran, the consumer's required input resolved to nothing, and the engine skipped
    /// the whole branch as dead while the run reported `ok`. `GetVariable` was hand-exempted when
    /// someone hit this; `GetCameraByName` wasn't, so it worked on cron/manual runs and quietly
    /// did nothing on event-driven ones.
    #[test]
    fn a_pure_source_runs_on_a_subrun_and_keeps_its_consumer_alive() {
        let dev = crate::devices::Device {
            id: "cam-1".into(),
            ip: "10.0.0.9".into(),
            xaddr: String::new(),
            name: "Hall".into(),
            manufacturer: None,
            model: None,
            username: None,
            password: None,
            rtsp_url: None,
            rtsp_sub_url: None,
            stream_mode: None,
        
            ..Default::default()
        };
        let (tx, _rx) = std::sync::mpsc::channel();
        let ctx = ToolContext::new_local(
            std::sync::Arc::new(std::sync::Mutex::new(vec![dev])),
            crate::db::Db::lazy("postgres://sentinel:sentinel@localhost/sentinel_test"),
            crate::tenancy::DEFAULT_TENANT_ID,
            std::sync::Arc::new(std::sync::Mutex::new(vec![])),
            None,
            tx,
        );

        let mut g = Graph::new("g");
        let entry = g.add_node(NodeKind::OnEvent, 0, 0);
        g.node_mut(entry).unwrap().config["event"] = json!("motion");
        // The orphan: no incoming edges at all — its value comes purely from config.
        let src = g.add_node(NodeKind::GetCameraByName, 0, 0);
        g.node_mut(src).unwrap().config["camera"] = json!("Hall");
        // A consumer whose `camera` input is REQUIRED and wired to that orphan.
        let sink = g.add_node(NodeKind::BreakCamera, 0, 0);
        g.add_edge(src, "camera", sink, "camera").unwrap();
        g.add_edge(entry, "exec_out", sink, "exec_in").unwrap();

        let outs = run_subpath_eventful(
            &g,
            &ctx,
            entry,
            PortValues::new(),
            &mut |_| {},
            &mut |_, _, _| {},
            &mut |_, _| {},
        )
        .unwrap();

        assert!(
            outs.contains_key(&src),
            "the orphan source must run on a sub-run (it used to fall back to seed_entries=false)"
        );
        assert!(
            outs.contains_key(&sink),
            "its consumer must not be skipped as a dead branch"
        );
    }

    #[test]
    fn if_node_runs_only_the_live_branch() {
        for (cond, want, skip) in [(true, "a", "b"), (false, "b", "a")] {
            let ctx = test_ctx();
            let mut g = Graph::new("g");
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
            assert!(
                !outs.contains_key(&skipped),
                "dead branch is skipped (cond={cond})"
            );
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
        let mut g = Graph::new("g");
        let show = g.add_node(NodeKind::ShowImage, 0, 0);
        g.node_mut(show).unwrap().config =
            json!({ "label": "cam", "image": path.to_str().unwrap() });
        // run_orphans so the single, edgeless node participates in the run.
        g.run_orphans = true;
        run_dag(&g, &ctx).unwrap();

        match rx.try_recv() {
            Ok(ToolUiEvent::ShowImage {
                graph,
                node,
                label,
                jpeg,
            }) => {
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
        let mut g = Graph::new("g");
        let a = g.add_node(NodeKind::Format, 0, 0);
        let b = g.add_node(NodeKind::Format, 0, 0);
        g.add_edge(a, "exec_out", b, "exec_in").unwrap();
        let outs = run_dag(&g, &ctx).unwrap();
        assert!(outs.contains_key(&a) && outs.contains_key(&b));
    }

    #[test]
    fn exec_back_edge_loops_until_step_budget() {
        // A -exec-> B -exec(back)-> A, both memoize=false → infinite loop, bounded by the
        // step budget (it must error, not hang).
        let ctx = test_ctx();
        let mut g = Graph::new("loop");
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
        let mut g = Graph::new("loop");
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
        let mut g = Graph::new("g");
        let a = g.add_node(NodeKind::Format, 0, 0); // text out
        let b = g.add_node(NodeKind::Format, 0, 0);
        g.add_edge(a, "exec_out", b, "exec_in").unwrap();
        // exec back-edge → allowed (a loop).
        assert!(g.add_edge(b, "exec_out", a, "exec_in").is_ok());
        // data back-edge (b.text → a.p1) would create a data cycle → rejected.
        let err = g.add_edge(b, "text", a, "p1").unwrap_err();
        assert!(err.contains("data cycle"), "got: {err}");
    }

    #[test]
    fn add_edge_accepts_exec_fan_in_but_rejects_duplicate_pins() {
        let mut g = Graph::new("g");
        let a = g.add_node(NodeKind::Format, 0, 0);
        let b = g.add_node(NodeKind::Format, 0, 0);
        let c = g.add_node(NodeKind::Format, 0, 0);
        g.add_edge(a, "exec_out", c, "exec_in").unwrap();
        // A second, different source converging on the same exec_in → allowed (fan-in).
        assert!(g.add_edge(b, "exec_out", c, "exec_in").is_ok());
        // The same pin pair again → rejected, so fan-in can't stack duplicate wires.
        let err = g.add_edge(a, "exec_out", c, "exec_in").unwrap_err();
        assert!(err.contains("already wired"), "got: {err}");
        assert_eq!(g.edges.len(), 2);
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
        let mut g = Graph::new("loop");
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
        assert_eq!(
            acts.iter().filter(|&&x| x == body).count(),
            3,
            "body runs once per element"
        );
        assert_eq!(
            acts.iter().filter(|&&x| x == done).count(),
            1,
            "completed fires once"
        );
        assert_eq!(
            acts.iter().filter(|&&x| x == fe).count(),
            4,
            "fe: 3 elements + final"
        );
        let body_logs: Vec<String> = logs
            .iter()
            .filter(|(id, _)| *id == body)
            .map(|(_, m)| m.clone())
            .collect();
        assert_eq!(body_logs, vec!["→ a", "→ b", "→ c"], "item flows in order");
    }

    /// The real graph this bug was found in ("Face Test", tenant workflow 5ef37805). Shape copied
    /// from production: `When Event` fans out to four `Set Variable`s, the numeric ones drive
    /// `>=` → `And` → `Branch`, and the branch's true arm feeds a consumer whose input comes from a
    /// `Get Variable` — a Get created early in the editor, so it carries a LOWER node id (19) than
    /// the Set that fills it (27).
    ///
    /// Before the fix the Get won the topological tie-break, read an unset variable, and the
    /// consumer reported `missing required input` — while `bbox`, whose `zero_value` is a real
    /// all-zero box rather than `None`, was silently filled with garbage instead of erroring.
    /// Uses Text so the test needs no camera, sidecar or event payload; the ordering is the bug.
    #[test]
    fn face_test_graph_shape_get_before_set_reads_the_written_value() {
        let ctx = test_ctx();
        let mut g = Graph::new("Face Test");

        // Created FIRST, exactly as in the real graph → lowest ids, wins the tie-break.
        let get_image = g.add_node(NodeKind::GetVariable, 0, 0);
        g.node_mut(get_image).unwrap().config["variable"] = json!("image");
        g.node_mut(get_image).unwrap().config["type"] = json!("text");

        // The event's payload stand-in + the four Sets it fans out to.
        let entry = g.add_node(NodeKind::Format, 0, 0);
        g.node_mut(entry).unwrap().config["template"] = json!("FRAME");
        let set_image = g.add_node(NodeKind::SetVariable, 0, 0);
        g.node_mut(set_image).unwrap().config["variable"] = json!("image");
        g.node_mut(set_image).unwrap().config["type"] = json!("text");
        g.add_edge(entry, "exec_out", set_image, "exec_in").unwrap();
        g.add_edge(entry, "text", set_image, "value").unwrap();

        // face_width >= 20 AND face_confidence >= 60 → Branch, as in the real graph.
        let gte_w = g.add_node(NodeKind::NumGte, 0, 0);
        g.node_mut(gte_w).unwrap().config["a"] = json!(33);
        g.node_mut(gte_w).unwrap().config["b"] = json!(20);
        let gte_c = g.add_node(NodeKind::NumGte, 0, 0);
        g.node_mut(gte_c).unwrap().config["a"] = json!(89);
        g.node_mut(gte_c).unwrap().config["b"] = json!(60);
        let and = g.add_node(NodeKind::And, 0, 0);
        g.add_edge(gte_w, "result", and, "a").unwrap();
        g.add_edge(gte_c, "result", and, "b").unwrap();
        let branch = g.add_node(NodeKind::If, 0, 0);
        g.add_edge(and, "result", branch, "condition").unwrap();
        g.add_edge(set_image, "exec_out", branch, "exec_in")
            .unwrap();

        // The gated consumer, fed from the Get — the node that reported the missing input.
        let consumer = g.add_node(NodeKind::Format, 0, 0);
        g.node_mut(consumer).unwrap().config["template"] = json!("{p1}");
        g.add_edge(branch, "true", consumer, "exec_in").unwrap();
        g.add_edge(get_image, "value", consumer, "p1").unwrap();

        assert!(
            get_image < set_image,
            "the Get must hold the lower id — that is the condition that triggered the bug"
        );

        let outs = run_dag(&g, &ctx).expect("run");
        assert_eq!(
            outs.get(&consumer)
                .and_then(|m| m.get("text"))
                .and_then(PortValue::as_text),
            Some("FRAME"),
            "the gated consumer must see what the Set wrote, not an empty epoch-0 read"
        );
    }

    /// The same Set→Get contract, but with the Get created FIRST so it holds the lower node id.
    /// `set_variable_then_get_variable_reads_the_value` builds Set first and so passes on the
    /// topological tie-break (lowest id wins) rather than on the wiring — it would pass even if
    /// ordering were broken. This is the case that actually bit: a real graph whose Get was node 19
    /// and whose Set was node 27 read the variable before it was written.
    #[test]
    fn get_created_before_set_still_reads_the_written_value() {
        let ctx = test_ctx();
        let mut g = Graph::new("vars-id-order");
        // Get FIRST → lowest id → wins the tie-break → would run before Set without the fix.
        let get = g.add_node(NodeKind::GetVariable, 0, 0);
        g.node_mut(get).unwrap().config["variable"] = json!("greeting");
        g.node_mut(get).unwrap().config["type"] = json!("text");
        let src = g.add_node(NodeKind::Format, 0, 0);
        g.node_mut(src).unwrap().config["template"] = json!("hello");
        let set = g.add_node(NodeKind::SetVariable, 0, 0);
        g.node_mut(set).unwrap().config["variable"] = json!("greeting");
        g.node_mut(set).unwrap().config["type"] = json!("text");
        let sink = g.add_node(NodeKind::Format, 0, 0);
        g.node_mut(sink).unwrap().config["template"] = json!("{p1}");
        g.add_edge(src, "exec_out", set, "exec_in").unwrap();
        g.add_edge(src, "text", set, "value").unwrap();
        g.add_edge(set, "exec_out", sink, "exec_in").unwrap();
        g.add_edge(get, "value", sink, "p1").unwrap();

        let outs = run_dag(&g, &ctx).expect("run");
        let got = outs
            .get(&sink)
            .and_then(|m| m.get("text"))
            .and_then(PortValue::as_text);
        assert_eq!(
            got,
            Some("hello"),
            "the Get must read what the Set wrote — ordering follows the wires, not node ids"
        );
    }

    #[test]
    fn set_variable_then_get_variable_reads_the_value() {
        // Format("hello") --exec--> SetVariable(greeting)   [value <- Format.text]
        //                            SetVariable --exec--> Format("{p1}")  [p1 <- GetVariable.value]
        // The Get must read "hello" (the value Set wrote), not its epoch-0 zero default.
        let ctx = test_ctx();
        let mut g = Graph::new("vars");
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
        let got = outs
            .get(&sink)
            .and_then(|m| m.get("text"))
            .and_then(PortValue::as_text);
        assert_eq!(got, Some("hello"), "Get reads the value Set wrote");
    }

    #[test]
    fn get_variable_logs_only_on_the_branch_that_ran() {
        // #115: a pure `GetVariable` activates every run, but its log line + canvas highlight
        // (both driven by `on_log`) must only appear when a node it feeds actually ran. Build a
        // two-branch graph: `If(false)` lights the `false` branch; each branch has its own `Get`
        // feeding its branch's `Format`. Only the `false`-branch Get should log.
        let ctx = test_ctx();
        let mut g = Graph::new("g");
        g.debug = true;

        let gate = g.add_node(NodeKind::If, 0, 0);
        g.node_mut(gate).unwrap().config["condition"] = json!(false); // → false branch runs

        // TRUE branch (does NOT run): Get feeds the true-branch Format.
        let get_a = g.add_node(NodeKind::GetVariable, 0, 0);
        g.node_mut(get_a).unwrap().config["variable"] = json!("a");
        g.node_mut(get_a).unwrap().config["type"] = json!("text");
        let fmt_a = g.add_node(NodeKind::Format, 0, 0);
        g.node_mut(fmt_a).unwrap().config["template"] = json!("{p1}");
        g.add_edge(gate, "true", fmt_a, "exec_in").unwrap();
        g.add_edge(get_a, "value", fmt_a, "p1").unwrap();

        // FALSE branch (runs): Get feeds the false-branch Format.
        let get_b = g.add_node(NodeKind::GetVariable, 0, 0);
        g.node_mut(get_b).unwrap().config["variable"] = json!("b");
        g.node_mut(get_b).unwrap().config["type"] = json!("text");
        let fmt_b = g.add_node(NodeKind::Format, 0, 0);
        g.node_mut(fmt_b).unwrap().config["template"] = json!("{p1}");
        g.add_edge(gate, "false", fmt_b, "exec_in").unwrap();
        g.add_edge(get_b, "value", fmt_b, "p1").unwrap();

        let (_, logs) = run_capture(&g, &ctx);
        let logged: HashSet<u32> = logs.iter().map(|(id, _)| *id).collect();
        // Both Gets ACTIVATE (pure sources), but only the one feeding the branch that ran logs.
        assert!(
            logged.contains(&get_b),
            "the Get feeding the branch that ran must log"
        );
        assert!(
            !logged.contains(&get_a),
            "the Get feeding the dead branch must NOT log/highlight (#115)"
        );
        // Sanity: the false branch's Format ran, the true branch's did not.
        assert!(logged.contains(&fmt_b) && !logged.contains(&fmt_a));
    }

    #[test]
    fn get_variable_with_no_consumers_stays_quiet() {
        // A truly unused variable read (no outgoing edge) never lit a node → no log/highlight.
        let ctx = test_ctx();
        let mut g = Graph::new("g");
        g.debug = true;
        g.run_orphans = true; // let the edgeless Get participate in the run
        let get = g.add_node(NodeKind::GetVariable, 0, 0);
        g.node_mut(get).unwrap().config["variable"] = json!("x");
        g.node_mut(get).unwrap().config["type"] = json!("text");
        let (_, logs) = run_capture(&g, &ctx);
        assert!(
            !logs.iter().any(|(id, _)| *id == get),
            "a Get with no consumers stays quiet"
        );
    }

    #[test]
    fn get_variable_feeding_a_node_that_ran_still_logs() {
        // A normal single-branch graph is unaffected: the Get feeds a Format that runs → it logs.
        let ctx = test_ctx();
        let mut g = Graph::new("g");
        g.debug = true;
        let get = g.add_node(NodeKind::GetVariable, 0, 0);
        g.node_mut(get).unwrap().config["variable"] = json!("greeting");
        g.node_mut(get).unwrap().config["type"] = json!("text");
        let fmt = g.add_node(NodeKind::Format, 0, 0);
        g.node_mut(fmt).unwrap().config["template"] = json!("{p1}");
        g.add_edge(get, "value", fmt, "p1").unwrap();
        let (_, logs) = run_capture(&g, &ctx);
        assert!(
            logs.iter().any(|(id, _)| *id == get),
            "a Get feeding a node that ran still logs"
        );
    }

    #[test]
    fn for_each_empty_list_only_fires_completed() {
        let ctx = test_ctx();
        let mut g = Graph::new("loop");
        let fe = g.add_node(NodeKind::ForEach, 0, 0); // items defaults to ""
        let body = g.add_node(NodeKind::Format, 0, 0);
        g.node_mut(body).unwrap().memoize = false;
        g.add_edge(fe, "loop_body", body, "exec_in").unwrap();
        let done = g.add_node(NodeKind::Format, 0, 0);
        g.add_edge(fe, "completed", done, "exec_in").unwrap();

        let (acts, _) = run_capture(&g, &ctx);
        assert_eq!(
            acts.iter().filter(|&&x| x == body).count(),
            0,
            "no body on empty list"
        );
        assert_eq!(
            acts.iter().filter(|&&x| x == done).count(),
            1,
            "completed fires once"
        );
    }

    #[test]
    fn for_each_iterates_a_json_number_array() {
        // A JSON number array yields Num elements (not text). Verify the body runs once per
        // element (3×) — the element type is checked directly in `parse_text_items_typing`.
        let ctx = test_ctx();
        let mut g = Graph::new("loop");
        let fe = g.add_node(NodeKind::ForEach, 0, 0);
        g.node_mut(fe).unwrap().config["items"] = json!("[1, 2, 3]");
        let body = g.add_node(NodeKind::Print, 0, 0);
        g.node_mut(body).unwrap().memoize = false;
        g.add_edge(fe, "loop_body", body, "exec_in").unwrap();
        let (acts, _) = run_capture(&g, &ctx);
        assert_eq!(
            acts.iter().filter(|&&x| x == body).count(),
            3,
            "3 JSON elements iterate"
        );
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
        let mut g = Graph::new("g");
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
        let outs = run_dag_eventful(&g, &ctx, &mut |_| {}, &mut |_, _, _| {}, &mut |ev, _| {
            fired.push(ev.to_string())
        })
        .unwrap();
        assert_eq!(fired, vec!["seen"], "EmitEvent calls the emit sink");
        assert!(outs.contains_key(&em), "EmitEvent ran");
        assert!(
            !outs.contains_key(&on),
            "OnEvent must NOT run on a normal trigger"
        );
        assert!(
            !outs.contains_key(&fmt),
            "the OnEvent handler is inert on a normal run"
        );

        // The handler runs only when its event fires (a sub-run seeded at the OnEvent node).
        let entry = on_event_node(&g, "seen", None, &[]).expect("OnEvent found by name");
        assert_eq!(entry, on);
        let outs2 = run_subpath_eventful(
            &g,
            &ctx,
            entry,
            PortValues::new(),
            &mut |_| {},
            &mut |_, _, _| {},
            &mut |_, _| {},
        )
        .unwrap();
        assert!(
            outs2.contains_key(&on) && outs2.contains_key(&fmt),
            "handler subtree runs on its event"
        );
        assert!(
            !outs2.contains_key(&em),
            "the sub-run runs ONLY the handler subtree"
        );
    }

    /// Two test cameras: "hall" (10.0.0.1) and "lobby" (10.0.0.2) — mirrors
    /// `test_ctx_with_cameras`'s fixtures, just as a plain `Vec<Device>` (no `ToolContext` needed
    /// for graph-static camera resolution).
    fn two_test_devices() -> Vec<crate::devices::Device> {
        use crate::devices::Device;
        vec![
            Device {
                id: "urn:uuid:aaa".into(),
                ip: "10.0.0.1".into(),
                xaddr: "".into(),
                name: "hall".into(),
                manufacturer: None,
                model: None,
                username: None,
                password: None,
                rtsp_url: None,
                rtsp_sub_url: None,
                stream_mode: None,
                ..Default::default()
            },
            Device {
                id: "".into(),
                ip: "10.0.0.2".into(),
                xaddr: "".into(),
                name: "lobby".into(),
                manufacturer: None,
                model: None,
                username: None,
                password: None,
                rtsp_url: None,
                rtsp_sub_url: None,
                stream_mode: None,
                ..Default::default()
            },
        ]
    }

    #[test]
    fn on_event_node_camera_gate_scoped_vs_unscoped() {
        let devices = two_test_devices();
        let mut g = Graph::new("g");
        // Scoped: OnEvent("object_entered") with camera="hall" (a plain config literal, no wire).
        let scoped = g.add_node(NodeKind::OnEvent, 0, 0);
        g.node_mut(scoped).unwrap().config = json!({ "event": "object_entered", "camera": "hall" });
        // Fires for hall's ip, not lobby's.
        assert_eq!(
            on_event_node(&g, "object_entered", Some("10.0.0.1"), &devices),
            Some(scoped)
        );
        assert_eq!(
            on_event_node(&g, "object_entered", Some("10.0.0.2"), &devices),
            None
        );

        // Unscoped (no camera set) matches any camera — today's behaviour, unchanged.
        let mut g2 = Graph::new("g2");
        let unscoped = g2.add_node(NodeKind::OnEvent, 0, 0);
        g2.node_mut(unscoped).unwrap().config = json!({ "event": "object_entered" });
        assert_eq!(
            on_event_node(&g2, "object_entered", Some("10.0.0.1"), &devices),
            Some(unscoped)
        );
        assert_eq!(
            on_event_node(&g2, "object_entered", Some("10.0.0.2"), &devices),
            Some(unscoped)
        );

        // An event with no camera concept (event_camera=None) never filters, even on a scoped node.
        assert_eq!(
            on_event_node(&g, "object_entered", None, &devices),
            Some(scoped)
        );
    }

    #[test]
    fn on_event_node_camera_wired_from_get_camera_by_name() {
        let devices = two_test_devices();
        let mut g = Graph::new("g");
        let cam = g.add_node(NodeKind::GetCameraByName, 0, 0);
        g.node_mut(cam).unwrap().config = json!({ "camera": "lobby" });
        let on = g.add_node(NodeKind::OnEvent, 0, 0);
        g.node_mut(on).unwrap().config = json!({ "event": "object_exited" });
        g.add_edge(cam, "camera", on, "camera").unwrap();

        assert_eq!(
            on_event_node(&g, "object_exited", Some("10.0.0.2"), &devices),
            Some(on),
            "resolves the wired GetCameraByName, not a literal"
        );
        assert_eq!(
            on_event_node(&g, "object_exited", Some("10.0.0.1"), &devices),
            None
        );
    }

    #[test]
    fn event_payload_makes_dynamic_ports_and_seeds_the_on_event_output() {
        let ctx = test_ctx();
        let mut g = Graph::new("g");
        // An event "seen" carrying a Text field "msg" — baked onto both nodes' config.
        let schema = json!({ "event": "seen", "payload": [{ "name": "msg", "type": "text" }] });
        let em = g.add_node(NodeKind::EmitEvent, 0, 0);
        g.node_mut(em).unwrap().config = schema.clone();
        let on = g.add_node(NodeKind::OnEvent, 0, 0);
        g.node_mut(on).unwrap().config = schema;

        // EmitEvent gains a Text INPUT port "msg"; OnEvent a Text OUTPUT port "msg".
        assert_eq!(
            g.node(em).unwrap().input_port("msg").unwrap().ty,
            PortType::Text
        );
        assert_eq!(
            g.node(on).unwrap().output_port("msg").unwrap().ty,
            PortType::Text
        );

        // The dynamic ports wire by type: Format.text → EmitEvent.msg, OnEvent.msg → Format.p1.
        let src = g.add_node(NodeKind::Format, 0, 0);
        g.add_edge(src, "text", em, "msg")
            .expect("payload INPUT port wires by type");
        let sink = g.add_node(NodeKind::Format, 0, 0);
        g.add_edge(on, "exec_out", sink, "exec_in").unwrap();
        g.add_edge(on, "msg", sink, "p1")
            .expect("payload OUTPUT port wires by type");

        // Firing the handler with a payload seeds the OnEvent's "msg" output for downstream.
        let entry = on_event_node(&g, "seen", None, &[]).unwrap();
        let mut payload = PortValues::new();
        payload.insert("msg".into(), PortValue::Text("hello".into()));
        let outs = run_subpath_eventful(
            &g,
            &ctx,
            entry,
            payload,
            &mut |_| {},
            &mut |_, _, _| {},
            &mut |_, _| {},
        )
        .unwrap();
        assert_eq!(
            outs.get(&on)
                .and_then(|m| m.get("msg"))
                .and_then(PortValue::as_text),
            Some("hello"),
            "OnEvent exposes the delivered payload on its named output"
        );
    }

    #[test]
    fn concurrency_policies_spawn_queue_skip() {
        let t = uuid::Uuid::nil();
        let mut e = WorkflowEngine::new();
        let p = ConcurrencyPolicy::SkipIfRunning;
        assert_eq!(e.decide(t, uuid::Uuid::from_u128(1), &p), Fire::Spawn);
        e.on_spawn(t, uuid::Uuid::from_u128(1));
        assert_eq!(e.decide(t, uuid::Uuid::from_u128(1), &p), Fire::Skip);
        assert!(!e.on_finish(t, uuid::Uuid::from_u128(1)));
        assert_eq!(e.decide(t, uuid::Uuid::from_u128(1), &p), Fire::Spawn);

        let mut q = WorkflowEngine::new();
        let qp = ConcurrencyPolicy::QueueSingle { max_queued: 1 };
        assert_eq!(q.decide(t, uuid::Uuid::from_u128(1), &qp), Fire::Spawn);
        q.on_spawn(t, uuid::Uuid::from_u128(1));
        assert_eq!(q.decide(t, uuid::Uuid::from_u128(1), &qp), Fire::Queue);
        q.on_queue(t, uuid::Uuid::from_u128(1));
        assert_eq!(q.decide(t, uuid::Uuid::from_u128(1), &qp), Fire::Skip);
        assert!(q.on_finish(t, uuid::Uuid::from_u128(1)));

        let mut c = WorkflowEngine::new();
        let cp = ConcurrencyPolicy::Concurrent { max_concurrent: 2 };
        assert_eq!(c.decide(t, uuid::Uuid::from_u128(2), &cp), Fire::Spawn);
        c.on_spawn(t, uuid::Uuid::from_u128(2));
        assert_eq!(c.decide(t, uuid::Uuid::from_u128(2), &cp), Fire::Spawn);
        c.on_spawn(t, uuid::Uuid::from_u128(2));
        assert_eq!(c.decide(t, uuid::Uuid::from_u128(2), &cp), Fire::Skip);
    }

    #[test]
    fn same_graph_id_under_different_tenants_is_isolated() {
        // Graph id 1 of tenant A must not share run/cron state with graph id 1 of tenant B.
        let a = uuid::Uuid::from_u128(1);
        let b = uuid::Uuid::from_u128(2);
        let mut e = WorkflowEngine::new();
        let p = ConcurrencyPolicy::SkipIfRunning;
        e.on_spawn(a, uuid::Uuid::from_u128(1));
        assert_eq!(
            e.decide(a, uuid::Uuid::from_u128(1), &p),
            Fire::Skip,
            "A's graph 1 is running"
        );
        assert_eq!(
            e.decide(b, uuid::Uuid::from_u128(1), &p),
            Fire::Spawn,
            "B's graph 1 is independent"
        );
        let base = Instant::now();
        e.seed_cron_last(a, uuid::Uuid::from_u128(1), base);
        assert!(!e.cron_due(
            a,
            uuid::Uuid::from_u128(1),
            30,
            base + Duration::from_secs(10)
        ));
        assert!(
            e.cron_due(
                b,
                uuid::Uuid::from_u128(1),
                30,
                base + Duration::from_secs(10)
            ),
            "B's cron is independent"
        );
    }

    #[test]
    fn ask_ai_routes_true_branch_on_yes_answer() {
        let ctx = ctx_with_vision(true);
        let mut g = Graph::new("g");
        let ai = g.add_node(NodeKind::AskAi, 0, 0);
        g.node_mut(ai).unwrap().config =
            json!({ "question": "Is someone there?", "timeout_secs": "65" });
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
        let mut g = Graph::new("g");
        let ai = g.add_node(NodeKind::AskAi, 0, 0);
        g.node_mut(ai).unwrap().config =
            json!({ "question": "Is someone there?", "timeout_secs": "65" });
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
        let mut g = Graph::new("g");
        let ai = g.add_node(NodeKind::AskAi, 0, 0);
        // No question in config or wired
        g.node_mut(ai).unwrap().config = json!({ "question": "" });
        g.run_orphans = true;
        let err = run_dag(&g, &ctx).unwrap_err();
        assert!(err.contains("question is required"), "got: {err}");
    }

    #[test]
    fn ask_ai_wires_true_false_exec_ports() {
        let mut g = Graph::new("g");
        let ai = g.add_node(NodeKind::AskAi, 0, 0);
        let t = g.add_node(NodeKind::Format, 0, 0);
        let f = g.add_node(NodeKind::Format, 0, 0);
        g.add_edge(ai, "true", t, "exec_in")
            .expect("true port wires");
        g.add_edge(ai, "false", f, "exec_in")
            .expect("false port wires");
        // image and question wire correctly
        let snap = g.add_node(NodeKind::CameraSnapshot, 0, 0);
        g.add_edge(snap, "image", ai, "image").expect("image wires");
    }

    #[test]
    fn string_switch_routes_to_matching_arm() {
        // StringSwitch("red") → fires the "red" arm; "blue" and "default" are skipped.
        let ctx = test_ctx();
        let mut g = Graph::new("g");
        let sw = g.add_node(NodeKind::StringSwitch, 0, 0);
        g.node_mut(sw).unwrap().config =
            json!({ "arms": ["red", "blue", "green"], "value": "red" });
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
        assert!(
            !outs.contains_key(&def),
            "'default' skipped when a match fires"
        );
    }

    #[test]
    fn string_switch_fires_default_when_no_arm_matches() {
        // StringSwitch("purple") → no arm matches → "default" fires, named arms skip.
        let ctx = test_ctx();
        let mut g = Graph::new("g");
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
        let mut g = Graph::new("g");
        let fmt = g.add_node(NodeKind::Format, 0, 0);
        g.node_mut(fmt).unwrap().config =
            json!({ "template": "blue", "p1": "", "p2": "", "p3": "" });
        let sw = g.add_node(NodeKind::StringSwitch, 0, 0);
        g.node_mut(sw).unwrap().config = json!({ "arms": ["red", "blue", "green"] });
        g.add_edge(fmt, "exec_out", sw, "exec_in").unwrap();
        g.add_edge(fmt, "text", sw, "value").unwrap();
        let blue = g.add_node(NodeKind::Format, 0, 0);
        let def = g.add_node(NodeKind::Format, 0, 0);
        g.add_edge(sw, "blue", blue, "exec_in").unwrap();
        g.add_edge(sw, "default", def, "exec_in").unwrap();
        let outs = run_dag(&g, &ctx).unwrap();
        assert!(
            outs.contains_key(&blue),
            "'blue' arm fires (wired value = \"blue\")"
        );
        assert!(!outs.contains_key(&def), "'default' skipped");
    }

    #[test]
    fn string_switch_missing_value_errors() {
        // No wired value and no config literal → run errors on missing required input.
        let ctx = test_ctx();
        let mut g = Graph::new("g");
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
        let cam_a = Device {
            id: "urn:uuid:aaa".into(),
            ip: "10.0.0.1".into(),
            xaddr: "".into(),
            name: "hall".into(),
            manufacturer: None,
            model: None,
            username: None,
            password: None,
            rtsp_url: None,
            rtsp_sub_url: None,
            stream_mode: None,
        
            ..Default::default()
        };
        let cam_b = Device {
            id: "".into(),
            ip: "10.0.0.2".into(),
            xaddr: "".into(),
            name: "lobby".into(),
            manufacturer: None,
            model: None,
            username: None,
            password: None,
            rtsp_url: None,
            rtsp_sub_url: None,
            stream_mode: None,
        
            ..Default::default()
        };
        let (tx, _rx) = std::sync::mpsc::channel();
        ToolContext::new_local(
            std::sync::Arc::new(std::sync::Mutex::new(vec![cam_a, cam_b])),
            crate::db::Db::lazy("postgres://x@localhost/x"),
            crate::tenancy::DEFAULT_TENANT_ID,
            std::sync::Arc::new(std::sync::Mutex::new(vec![])),
            None,
            tx,
        )
    }

    // Build a graph with a CameraSwitch node that gets the camera from a literal in config.
    // The engine resolves `config["camera"]` as a camera entity (via resolve_camera_literal).
    fn make_camera_switch_graph(
        camera_name: &str,
        arms: serde_json::Value,
    ) -> (Graph, u32, u32, u32, u32) {
        let mut g = Graph::new("g");
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
        let (g, sw, arm1, arm2, def) =
            make_camera_switch_graph("hall", json!(["urn:uuid:aaa", "10.0.0.2"]));
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
        let (g, _sw, arm1, arm2, def) =
            make_camera_switch_graph("lobby", json!(["urn:uuid:aaa", "10.0.0.2"]));
        let outs = run_dag(&g, &ctx).unwrap();
        assert!(!outs.contains_key(&arm1), "arm_1 skipped");
        assert!(outs.contains_key(&arm2), "arm_2 (lobby by ip) fires");
        assert!(!outs.contains_key(&def), "default skipped");
    }

    #[test]
    fn camera_switch_fires_default_when_no_arm_matches() {
        // Camera ip/id not in any arm → default fires; named arm skips.
        let ctx = test_ctx_with_cameras();
        let mut g = Graph::new("g");
        g.run_orphans = true;
        let sw = g.add_node(NodeKind::CameraSwitch, 0, 0);
        g.node_mut(sw).unwrap().config = json!({ "arms": ["10.0.0.99"], "camera": "hall" });
        let arm1 = g.add_node(NodeKind::Format, 0, 0);
        let def = g.add_node(NodeKind::Format, 0, 0);
        g.add_edge(sw, "arm_1", arm1, "exec_in").unwrap();
        g.add_edge(sw, "default", def, "exec_in").unwrap();
        let outs = run_dag(&g, &ctx).unwrap();
        assert!(!outs.contains_key(&arm1), "arm_1 skipped (no match)");
        assert!(
            outs.contains_key(&def),
            "default fires when nothing matches"
        );
    }

    #[test]
    fn camera_switch_empty_arm_string_never_matches() {
        // Empty string arms (old default config ["", ""]) must never match any camera —
        // the bug: `"" == cam.id` was true for legacy cameras with no ONVIF id.
        let ctx = test_ctx_with_cameras();
        let (g, _sw, arm1, _arm2, def) = make_camera_switch_graph(
            "lobby",
            json!(["", ""]), // lobby has id="" — must NOT false-match
        );
        let outs = run_dag(&g, &ctx).unwrap();
        assert!(
            !outs.contains_key(&arm1),
            "empty arm must not false-match a legacy camera (id=\"\")"
        );
        assert!(outs.contains_key(&def), "default fires for unset arms");
    }

    #[test]
    fn seeded_cron_waits_out_remaining_interval() {
        // Phase 0.5: a restart seeds cron_last from the persisted fire time so a
        // 30s cron that fired mid-interval does NOT fire immediately on boot.
        let t = uuid::Uuid::nil();
        let mut e = WorkflowEngine::new();
        let base = Instant::now();
        e.seed_cron_last(t, uuid::Uuid::from_u128(1), base);
        // 10s after the last fire → not due yet.
        assert!(!e.cron_due(
            t,
            uuid::Uuid::from_u128(1),
            30,
            base + Duration::from_secs(10)
        ));
        // 40s after the last fire → due (and re-arms at that instant).
        assert!(e.cron_due(
            t,
            uuid::Uuid::from_u128(1),
            30,
            base + Duration::from_secs(40)
        ));
        // Just after re-arming → not due again.
        assert!(!e.cron_due(
            t,
            uuid::Uuid::from_u128(1),
            30,
            base + Duration::from_secs(50)
        ));
    }
}
