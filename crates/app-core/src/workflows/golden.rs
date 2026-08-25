//! Golden graphs — the execution oracle.
//!
//! The catalog snapshot (`catalog.rs`) freezes what a node *declares*. This freezes what a
//! graph *does*: for each shape, the serialized document is compared byte for byte against a
//! committed fixture, then the graph is run and the exact sequence of executed node ids is
//! asserted.
//!
//! Why both halves matter for the ggraph extraction. The serialization half catches a change to
//! the stored document — a renamed slug, a moved port, a field that stopped defaulting — which
//! is how a customer's saved graph stops loading. The execution half catches a change to the
//! scheduler: dead-branch skipping, loop re-entry, entry seeding. Neither is caught by
//! `cargo check`, and until #Fase 0 neither was caught by CI at all, because `check:rust` runs
//! `cargo check --all-targets`, which compiles the tests and never runs them.
//!
//! These shapes are hand-built rather than exported from production on purpose: a real graph
//! document carries camera names, LAN addresses and recipient labels, and those do not belong
//! in a git repository. The shapes are what matter, not the customer data in them.
//!
//! Regenerate the fixtures deliberately:
//!
//!     UPDATE_GOLDEN=1 cargo test -p app-core --lib workflows::golden

use super::domain::{Graph, NodeKind};
use super::node::{PortValue, PortValues};
use crate::agent::tools::ToolContext;
use serde_json::{json, Value};
use uuid::Uuid;

/// A fixed id so the serialized document is stable across runs (`Graph::new` mints a v4).
fn fixed_id(n: u8) -> Uuid {
    Uuid::from_bytes([n; 16])
}

fn graph(name: &str, id: u8) -> Graph {
    let mut g = Graph::new(name);
    g.id = fixed_id(id);
    g.enabled = true;
    g
}

fn cfg(g: &mut Graph, id: u32, v: Value) {
    let n = g
        .nodes
        .iter_mut()
        .find(|n| n.id == id)
        .expect("node exists");
    n.config = v;
}

fn wire(g: &mut Graph, from: u32, from_port: &str, to: u32, to_port: &str) {
    g.add_edge(from, from_port, to, to_port)
        .unwrap_or_else(|e| panic!("wire {from}.{from_port} -> {to}.{to_port}: {e}"));
}

// ---------------------------------------------------------------------------------------
// The shapes
// ---------------------------------------------------------------------------------------

/// A branch whose condition comes from a comparison, with a node on each arm.
///
/// Proves the two things a branch must do: route to the live arm, and leave the other arm's
/// subtree unexecuted. A scheduler that ran both arms would still report `ok`.
fn shape_branch() -> Graph {
    let mut g = graph("golden: branch", 1);
    let gt = g.add_node(NodeKind::NumGt, 0, 0);
    let br = g.add_node(NodeKind::If, 200, 0);
    let hot = g.add_node(NodeKind::Print, 400, -80);
    let cold = g.add_node(NodeKind::Print, 400, 80);
    cfg(&mut g, gt, json!({ "a": "5", "b": "3" }));
    cfg(
        &mut g,
        hot,
        json!({ "message": "hot", "on_screen": false, "level": "info" }),
    );
    cfg(
        &mut g,
        cold,
        json!({ "message": "cold", "on_screen": false, "level": "info" }),
    );
    wire(&mut g, gt, "result", br, "condition");
    wire(&mut g, br, "true", hot, "exec_in");
    wire(&mut g, br, "false", cold, "exec_in");
    g
}

/// A loop over a three-item list, with a node in the body and a node after it.
///
/// Proves loop re-entry and that `completed` fires exactly once, after the body — the shape
/// where an off-by-one in the epoch scheduler is invisible to every other test.
fn shape_for_each() -> Graph {
    let mut g = graph("golden: for each", 2);
    let each = g.add_node(NodeKind::ForEach, 0, 0);
    let body = g.add_node(NodeKind::Print, 250, -80);
    let done = g.add_node(NodeKind::Print, 250, 80);
    cfg(&mut g, each, json!({ "items": "alpha,beta,gamma" }));
    cfg(
        &mut g,
        body,
        json!({ "message": "item", "on_screen": false, "level": "info" }),
    );
    cfg(
        &mut g,
        done,
        json!({ "message": "done", "on_screen": false, "level": "info" }),
    );
    wire(&mut g, each, "loop_body", body, "exec_in");
    wire(&mut g, each, "completed", done, "exec_in");
    g
}

/// A switch with config-declared arms, fed by a formatted string.
///
/// Proves the dynamic-exec-arm path: `string_switch` declares no static outputs, its arms come
/// from `config.arms`, and `GraphNode::output_port` resolves them by label. That mechanism has
/// no other test, and the ggraph registry has to reproduce it exactly (`camera_switch` names
/// arms positionally instead — the wire format depends on both).
fn shape_switch() -> Graph {
    let mut g = graph("golden: switch", 3);
    let fmt = g.add_node(NodeKind::Format, 0, 0);
    let sw = g.add_node(NodeKind::StringSwitch, 250, 0);
    let a = g.add_node(NodeKind::Print, 500, -80);
    let b = g.add_node(NodeKind::Print, 500, 80);
    cfg(
        &mut g,
        fmt,
        json!({ "template": "{p1}", "p1": "beta", "p2": "", "p3": "" }),
    );
    cfg(&mut g, sw, json!({ "arms": ["alpha", "beta"] }));
    cfg(
        &mut g,
        a,
        json!({ "message": "took alpha", "on_screen": false, "level": "info" }),
    );
    cfg(
        &mut g,
        b,
        json!({ "message": "took beta", "on_screen": false, "level": "info" }),
    );
    wire(&mut g, fmt, "text", sw, "value");
    wire(&mut g, fmt, "exec_out", sw, "exec_in");
    wire(&mut g, sw, "alpha", a, "exec_in");
    wire(&mut g, sw, "beta", b, "exec_in");
    g
}

fn shapes() -> Vec<(&'static str, Graph)> {
    vec![
        ("branch", shape_branch()),
        ("for_each", shape_for_each()),
        ("switch", shape_switch()),
    ]
}

// ---------------------------------------------------------------------------------------
// Running them
// ---------------------------------------------------------------------------------------

pub(super) fn test_ctx() -> ToolContext {
    let (tx, _rx) = std::sync::mpsc::channel();
    ToolContext::new_local(
        std::sync::Arc::new(std::sync::Mutex::new(vec![])),
        // Never connected: `Db::lazy` only dials on first use and every `repo::*` call in the
        // engine swallows the failure, so these shapes run with no database.
        crate::db::Db::lazy("postgres://sentinel:sentinel@127.0.0.1:1/sentinel_golden"),
        crate::tenancy::DEFAULT_TENANT_ID,
        std::sync::Arc::new(std::sync::Mutex::new(vec![])),
        None,
        tx,
    )
}

/// The ids of the nodes that executed, in order. This is the whole behavioural contract.
fn run_order(g: &Graph) -> Vec<u32> {
    let sys = test_ctx();
    let mut order = Vec::new();
    let mut on_node = |id: u32| order.push(id);
    let mut on_arm = |_: u32, _: &str| {};
    let mut on_log = |_: u32, _: &str, _: u128| {};
    let mut emit = |_: &str, _: &PortValues| {};
    super::engine::run_dag_eventful(g, &sys, &mut on_node, &mut on_arm, &mut on_log, &mut emit)
        .unwrap_or_else(|e| panic!("graph {:?} failed to run: {e}", g.name));
    order
}

// ---------------------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------------------

const FIXTURES: &[(&str, &str)] = &[
    ("branch", include_str!("golden/branch.json")),
    ("for_each", include_str!("golden/for_each.json")),
    ("switch", include_str!("golden/switch.json")),
];

fn rendered(g: &Graph) -> String {
    let mut s = serde_json::to_string_pretty(g).expect("graph serializes");
    s.push('\n');
    s
}

#[test]
fn shapes_match_their_committed_documents() {
    for (name, g) in shapes() {
        let now = rendered(&g);
        if std::env::var("UPDATE_GOLDEN").is_ok() {
            let path = format!(
                "{}/src/workflows/golden/{name}.json",
                env!("CARGO_MANIFEST_DIR")
            );
            std::fs::write(&path, &now).expect("fixture is writable");
            continue;
        }
        let want = FIXTURES
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, s)| *s)
            .expect("fixture is registered");
        assert_eq!(
            now, want,
            "the serialized document for shape {name:?} changed. If intended, regenerate with \
             UPDATE_GOLDEN=1 cargo test -p app-core --lib workflows::golden"
        );
    }
}

/// A stored document must still load. This is the half that a renamed slug breaks.
#[test]
fn committed_documents_still_deserialize_and_run() {
    for (name, text) in FIXTURES {
        let g: Graph = serde_json::from_str(text)
            .unwrap_or_else(|e| panic!("fixture {name:?} no longer deserializes: {e}"));
        let from_builder = shapes()
            .into_iter()
            .find(|(n, _)| n == name)
            .map(|(_, g)| g)
            .expect("shape exists");
        assert_eq!(
            run_order(&g),
            run_order(&from_builder),
            "shape {name:?}: the stored document and the built graph run differently"
        );
    }
}

#[test]
fn a_branch_runs_only_the_live_arm() {
    let g = shape_branch();
    let order = run_order(&g);
    // 1 = num_gt, 2 = if, 3 = print("hot"), 4 = print("cold")
    assert_eq!(order, vec![1, 2, 3], "5 > 3 must take the true arm only");
    assert!(
        !order.contains(&4),
        "the dead arm ran — a branch that executes both arms still reports ok"
    );
}

#[test]
fn a_loop_runs_its_body_once_per_item_then_completes() {
    let g = shape_for_each();
    let order = run_order(&g);
    // 1 = for_each, 2 = print(body), 3 = print(done)
    let body = order.iter().filter(|id| **id == 2).count();
    assert_eq!(
        body, 3,
        "three items must run the body three times: {order:?}"
    );
    let done = order.iter().filter(|id| **id == 3).count();
    assert_eq!(done, 1, "completed must fire exactly once: {order:?}");
    assert_eq!(
        order.last(),
        Some(&3),
        "completed must fire after the body: {order:?}"
    );
}

#[test]
fn a_switch_takes_the_arm_matching_its_value() {
    let g = shape_switch();
    let order = run_order(&g);
    // 1 = format, 2 = string_switch, 3 = print(alpha), 4 = print(beta)
    assert!(
        order.contains(&4),
        "value 'beta' must take the beta arm: {order:?}"
    );
    assert!(
        !order.contains(&3),
        "the alpha arm must stay dead: {order:?}"
    );
}

/// A stored node still carries `"memoize": false` — 249 of them in production the day it was
/// removed. Serde ignores unknown fields by default, so they load; this asserts that rather than
/// trusting it, because the failure mode is every graph refusing to open at once.
#[test]
fn a_node_saved_before_memoize_was_removed_still_loads() {
    let stored = serde_json::json!({
        "id": 3,
        "kind": "print",
        "x": 10,
        "y": 20,
        "config": { "message": "hi" },
        "memoize": false
    });
    let n: super::domain::GraphNode =
        serde_json::from_value(stored).expect("a stored node must still deserialize");
    assert_eq!(n.id, 3);
    assert_eq!(n.kind, NodeKind::Print);
}

// ---------------------------------------------------------------------------------------
// The shape that #114/#115 were about
// ---------------------------------------------------------------------------------------

/// A pure source with **no incoming edges**, feeding a **required** input, entered as a
/// **sub-run**.
///
/// This is the shape that silently broke twice. An orphan falls back to `seed_entries`, which is
/// false for every `OnEvent`/timer sub-path, so the source never ran; its consumer's required
/// input then resolved to nothing and the engine skipped the whole branch as dead — while the
/// run reported `ok`. It worked on manual and cron runs and quietly did nothing on event-driven
/// ones, which is why it took two cards to find.
///
/// Card #205 proposes narrowing the unconditional activation of pure sources so they stop
/// lighting up branches nobody took. This is the oracle that narrowing needs: if it still
/// passes, the read path covers the incident and the activation was doing nothing else. If it
/// fails, the activation is load-bearing and the fix has to be shaped differently.
fn shape_orphan_source_subrun() -> (Graph, u32, u32, u32) {
    let mut g = graph("golden: orphan source in a sub-run", 4);
    let src = g.add_node(NodeKind::GetVariable, 0, 0);
    let switch = g.add_node(NodeKind::StringSwitch, 250, 0);
    let taken = g.add_node(NodeKind::Print, 500, 0);
    cfg(
        &mut g,
        src,
        json!({ "variable": "greeting", "type": "text" }),
    );
    cfg(&mut g, switch, json!({ "arms": ["hello", "other"] }));
    cfg(
        &mut g,
        taken,
        json!({ "message": "took hello", "on_screen": false, "level": "info" }),
    );
    wire(&mut g, src, "value", switch, "value");
    wire(&mut g, switch, "hello", taken, "exec_in");
    (g, src, switch, taken)
}

#[test]
fn an_orphan_pure_source_feeds_a_required_input_on_a_sub_run() {
    let (g, src, switch, taken) = shape_orphan_source_subrun();
    let sys = test_ctx();
    sys.variables
        .lock()
        .unwrap()
        .insert("greeting".into(), PortValue::Text("hello".into()));

    // A sub-run: entered at the switch, the way an OnEvent or a timer enters one. Nothing is
    // seeded from the graph's entry points, so the orphan source has to be reached some other
    // way or its consumer's required input goes missing.
    let mut order = Vec::new();
    let mut on_node = |id: u32| order.push(id);
    let mut on_log = |_: u32, _: &str, _: u128| {};
    let mut emit = |_: &str, _: &PortValues| {};
    super::engine::run_subpath_eventful(
        &g,
        &sys,
        switch,
        PortValues::new(),
        &mut on_node,
        &mut |_: u32, _: &str| {},
        &mut on_log,
        &mut emit,
    )
    .expect("the sub-run must not fail");

    assert!(
        order.contains(&taken),
        "the switch's `hello` arm must fire, which means its required `value` input resolved. \
         It did not: {order:?}. That is the #114/#115 failure — a branch skipped as dead while \
         the run reports ok."
    );
    let _ = src;
}

/// The same graph, run normally, still routes on the variable's value rather than on a default.
#[test]
fn the_orphan_source_is_read_at_the_moment_its_consumer_runs() {
    let (g, _src, switch, taken) = shape_orphan_source_subrun();
    let sys = test_ctx();
    sys.variables
        .lock()
        .unwrap()
        .insert("greeting".into(), PortValue::Text("other".into()));

    let mut order = Vec::new();
    let mut on_node = |id: u32| order.push(id);
    let mut on_log = |_: u32, _: &str, _: u128| {};
    let mut emit = |_: &str, _: &PortValues| {};
    super::engine::run_subpath_eventful(
        &g,
        &sys,
        switch,
        PortValues::new(),
        &mut on_node,
        &mut |_: u32, _: &str| {},
        &mut on_log,
        &mut emit,
    )
    .expect("runs");

    assert!(
        !order.contains(&taken),
        "the variable says `other`, so the `hello` arm must stay dead: {order:?}"
    );
}

/// The canvas highlight must follow the branch that ran, not every branch.
///
/// #115 fixed this for the Console line and stopped there, because the log and the highlight
/// come from different callbacks — `on_log` and `on_node` — and only the first was deferred.
/// The reported symptom (#205) was every `Get Variable` in a two-armed graph lighting up when
/// one arm ran.
///
/// Asserted against `on_node`, which is what `sentinel-flow` turns into the `workflow-node`
/// event the editor animates. Deliberately with `debug` OFF: a highlight is not a debug feature,
/// and the log deferral is gated on debug, so a test that left it on would pass for the wrong
/// reason.
#[test]
fn only_the_get_on_the_live_branch_lights_up() {
    let mut g = graph("golden: two arms, two gets", 5);
    g.debug = false;

    let gate = g.add_node(NodeKind::If, 0, 0);
    cfg(&mut g, gate, json!({ "condition": false }));

    let get_dead = g.add_node(NodeKind::GetVariable, 0, -120);
    cfg(&mut g, get_dead, json!({ "variable": "a", "type": "text" }));
    let fmt_dead = g.add_node(NodeKind::Format, 250, -120);
    cfg(
        &mut g,
        fmt_dead,
        json!({ "template": "{p1}", "p1": "", "p2": "", "p3": "" }),
    );

    let get_live = g.add_node(NodeKind::GetVariable, 0, 120);
    cfg(&mut g, get_live, json!({ "variable": "b", "type": "text" }));
    let fmt_live = g.add_node(NodeKind::Format, 250, 120);
    cfg(
        &mut g,
        fmt_live,
        json!({ "template": "{p1}", "p1": "", "p2": "", "p3": "" }),
    );

    wire(&mut g, gate, "true", fmt_dead, "exec_in");
    wire(&mut g, get_dead, "value", fmt_dead, "p1");
    wire(&mut g, gate, "false", fmt_live, "exec_in");
    wire(&mut g, get_live, "value", fmt_live, "p1");

    let order = run_order(&g);

    assert!(
        order.contains(&get_live),
        "the Get feeding the arm that ran must light up: {order:?}"
    );
    assert!(
        !order.contains(&get_dead),
        "the Get on the arm nobody took must NOT light up — this is #205, and it is what a \
         person sees on the canvas: {order:?}"
    );
    assert!(
        order.iter().position(|n| *n == get_live) < order.iter().position(|n| *n == fmt_live),
        "and it must light up BEFORE the node that reads it, not at the end of the run: {order:?}"
    );
}

/// Only the arm that fired is reported, and it names the edge control took.
///
/// The canvas lights an edge from its own source arm. Before this the engine reported nothing
/// about arms at all, so the editor lit every edge entering a live node — right by accident with
/// one incoming edge, and wrong the moment two branches converge on one node (#206).
#[test]
fn the_engine_reports_which_arm_fired_not_which_node_was_reached() {
    let mut g = graph("golden: two branches, one target", 6);

    let gate = g.add_node(NodeKind::If, 0, 0);
    cfg(&mut g, gate, json!({ "condition": false }));
    let target = g.add_node(NodeKind::Print, 300, 0);
    cfg(
        &mut g,
        target,
        json!({ "message": "reached", "on_screen": false, "level": "info" }),
    );
    // BOTH arms of the branch wire into the same node — the shape from the report.
    wire(&mut g, gate, "true", target, "exec_in");
    wire(&mut g, gate, "false", target, "exec_in");

    let sys = test_ctx();
    let mut arms: Vec<(u32, String)> = Vec::new();
    let mut on_node = |_: u32| {};
    let mut on_arm = |nid: u32, port: &str| arms.push((nid, port.to_string()));
    let mut on_log = |_: u32, _: &str, _: u128| {};
    let mut emit = |_: &str, _: &PortValues| {};
    super::engine::run_dag_eventful(&g, &sys, &mut on_node, &mut on_arm, &mut on_log, &mut emit)
        .expect("runs");

    let from_gate: Vec<&String> = arms
        .iter()
        .filter(|(n, _)| *n == gate)
        .map(|(_, p)| p)
        .collect();
    assert_eq!(
        from_gate,
        vec!["false"],
        "the condition is false, so only the false arm fired. Reporting both — or reporting the \
         node instead of the arm — is what made the canvas light both edges: {arms:?}"
    );
}
