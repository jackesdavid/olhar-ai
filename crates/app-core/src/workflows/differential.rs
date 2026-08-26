//! The same graph under both schedulers, compared node for node.
//!
//! This product still runs graphs with its own scheduler (`engine.rs`, 2600 lines). The engine
//! it now depends on has one too. Replacing the first with the second is the last large step of
//! the extraction, and it is the one step the catalog snapshot cannot help with: a catalog says
//! what a node *declares*, and this is about what a run *does*.
//!
//! So: build a graph, run it both ways, and require the same sequence of node executions. A
//! difference is either a bug in the engine's scheduler or a behaviour of this one that nobody
//! had written down — and both are things to know before the swap, not after.
//!
//! ## What this is not
//!
//! The conversion this needed is gone: this product's `Graph` IS the engine's, so both
//! schedulers read the same document rather than one reading a copy of it.
//!
//! It is not a claim that the schedulers agree in general. It is a growing corpus, and the
//! honest reading of a passing run is "these shapes agree", nothing wider. Shapes are added as
//! they are understood; [`KNOWN_DIFFERENCES`] lists the ones that do not agree yet and why,
//! rather than letting them sit as a quietly ignored failure.

use super::domain::Graph;
use super::host::{SentinelHost, SentinelMeta};
use super::node::PortValues;
use crate::agent::tools::ToolContext;
use crate::workflows::domain::GraphExt;
use ggraph_core::host::{Observer, UiEvent};
use ggraph_core::{Entry, RunOptions};
use std::sync::{Arc, Mutex};

/// Shapes that do not agree yet, and why. A shape listed here is skipped by the sweep rather
/// than silently failing it — a red test nobody can fix is a red test everybody learns to ignore.
const KNOWN_DIFFERENCES: &[(&str, &str)] = &[];

/// Records the order nodes executed in, which is the only thing being compared.
#[derive(Default)]
struct Order(Mutex<Vec<u32>>);

impl Observer for Order {
    fn node_started(&self, node: u32) {
        self.0.lock().unwrap().push(node);
    }
    fn ui(&self, _node: u32, _event: UiEvent) {}
}

/// This product's scheduler.
fn order_here(g: &Graph, sys: &ToolContext) -> Result<Vec<u32>, String> {
    let mut order = Vec::new();
    let mut on_node = |id: u32| order.push(id);
    let mut on_arm = |_: u32, _: &str| {};
    let mut on_log = |_: u32, _: &str, _: u128| {};
    let mut emit = |_: &str, _: &PortValues| {};
    super::engine::run_dag_eventful(g, sys, &mut on_node, &mut on_arm, &mut on_log, &mut emit)?;
    Ok(order)
}

/// The engine's scheduler, over the same graph and the same node implementations.
fn order_engine(g: &Graph, sys: ToolContext) -> Result<Vec<u32>, String> {
    let rec = Arc::new(Order::default());
    // The deferral of pure sources is this product's, not the engine's — the engine reports every
    // node it runs and lets the host decide what that is worth. Wrapping the recorder here is what
    // makes the comparison fair: the driver's `on_node` is ALREADY deferred, so without this the
    // two would differ on every graph containing a `GetVariable`, and the difference would be the
    // harness rather than the schedulers.
    let host = SentinelHost::watching(
        sys,
        Box::new(super::observer::DeferringObserver::new(
            g,
            RecRef(rec.clone()),
            |n| {
                crate::workflows::domain::AsKind::as_kind(&n.kind)
                    .is_some_and(|k| k.is_pure_source())
            },
        )),
    );
    ggraph_core::run(
        g,
        super::registry::registry(),
        &host,
        &Entry::default(),
        &RunOptions::default(),
    )
    .map_err(|e| e.to_string())?;
    let v = rec.0.lock().unwrap().clone();
    Ok(v)
}

/// `Box<dyn Observer>` needs an owned value; the test needs to read the recording afterwards.
struct RecRef(Arc<Order>);

impl Observer for RecRef {
    fn node_started(&self, node: u32) {
        self.0.node_started(node);
    }
    fn ui(&self, node: u32, event: UiEvent) {
        self.0.ui(node, event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workflows::domain::NodeKind;
    use crate::workflows::golden::test_ctx;
    use serde_json::json;

    /// Compare one shape, and report a difference the way a person needs to read it.
    fn agree(name: &str, g: &Graph) {
        if let Some((_, why)) = KNOWN_DIFFERENCES.iter().find(|(n, _)| *n == name) {
            eprintln!("skipping {name}: {why}");
            return;
        }
        let here = order_here(g, &test_ctx())
            .unwrap_or_else(|e| panic!("{name}: this product's scheduler failed: {e}"));
        let engine = order_engine(g, test_ctx())
            .unwrap_or_else(|e| panic!("{name}: the engine's scheduler failed: {e}"));
        assert_eq!(
            here, engine,
            "{name}: the two schedulers ran different nodes, or ran them in a different order.\n  \
             this product: {here:?}\n  engine:       {engine:?}\n\n\
             Either the engine's scheduler is wrong, or this one does something nobody wrote \
             down. Both are worth knowing BEFORE the swap. If the difference is understood and \
             intended, add the shape to KNOWN_DIFFERENCES with the reason rather than deleting \
             this assertion."
        );
    }

    fn g(name: &str) -> Graph {
        let mut g = Graph::new(name);
        g.meta.enabled = true;
        g
    }

    /// Two branches converging on one node. The shape that broke the edge highlight (#206), and
    /// the one where a scheduler that counts activations rather than arms goes wrong.
    #[test]
    fn fan_in_runs_the_target_once_per_live_arm() {
        let mut x = g("fan in");
        let gate = x.add(NodeKind::If, 0, 0);
        x.node_mut(gate).unwrap().config = json!({ "condition": true });
        let target = x.add(NodeKind::Print, 300, 0);
        x.node_mut(target).unwrap().config =
            json!({ "message": "reached", "level": "info", "on_screen": false });
        x.wire(gate, "true", target, "exec_in").unwrap();
        x.wire(gate, "false", target, "exec_in").unwrap();
        agree("fan in", &x);
    }

    /// A switch picks one arm by matching text, and the arms are named by configuration rather
    /// than by a static table — so this also checks both schedulers resolve the same arm SET.
    #[test]
    fn a_switch_takes_the_arm_its_value_names() {
        let mut x = g("switch");
        let src = x.add(NodeKind::Format, 0, 0);
        x.node_mut(src).unwrap().config = json!({ "template": "beta" });
        let sw = x.add(NodeKind::StringSwitch, 200, 0);
        x.node_mut(sw).unwrap().config = json!({ "arms": ["alpha", "beta"] });
        let hit = x.add(NodeKind::Print, 400, -60);
        x.node_mut(hit).unwrap().config =
            json!({ "message": "beta", "level": "info", "on_screen": false });
        let miss = x.add(NodeKind::Print, 400, 60);
        x.node_mut(miss).unwrap().config =
            json!({ "message": "default", "level": "info", "on_screen": false });
        x.wire(src, "text", sw, "value").unwrap();
        x.wire(sw, "beta", hit, "exec_in").unwrap();
        x.wire(sw, "default", miss, "exec_in").unwrap();
        agree("switch", &x);
    }

    /// Deadness has to PROPAGATE: everything under an untaken arm stays unrun, not just the
    /// first node. A scheduler that only checks the immediate parent gets this wrong two nodes
    /// down, which is exactly where nobody looks.
    #[test]
    fn a_dead_arm_stays_dead_all_the_way_down() {
        let mut x = g("deep dead");
        let gate = x.add(NodeKind::If, 0, 0);
        x.node_mut(gate).unwrap().config = json!({ "condition": false });
        let first = x.add(NodeKind::Print, 200, 0);
        x.node_mut(first).unwrap().config =
            json!({ "message": "one", "level": "info", "on_screen": false });
        let second = x.add(NodeKind::Print, 400, 0);
        x.node_mut(second).unwrap().config =
            json!({ "message": "two", "level": "info", "on_screen": false });
        x.wire(gate, "true", first, "exec_in").unwrap();
        x.wire(first, "exec_out", second, "exec_in").unwrap();
        agree("deep dead", &x);
    }

    /// A node whose inputs come only from the inspector, with nothing wired. The common case in
    /// a real graph, and the one the engine could not do at all until `Host::literals` existed.
    #[test]
    fn a_node_fed_only_by_configuration_runs() {
        let mut x = g("literals");
        let cmp = x.add(NodeKind::NumGt, 0, 0);
        x.node_mut(cmp).unwrap().config = json!({ "a": "5", "b": "3" });
        let br = x.add(NodeKind::If, 200, 0);
        let taken = x.add(NodeKind::Print, 400, 0);
        x.node_mut(taken).unwrap().config =
            json!({ "message": "greater", "level": "info", "on_screen": false });
        x.wire(cmp, "result", br, "condition").unwrap();
        x.wire(br, "true", taken, "exec_in").unwrap();
        agree("literals", &x);
    }

    /// A loop whose body is itself a loop. The index lives per node, so an inner loop finishing
    /// must not look finished when the outer one comes round again.
    #[test]
    fn a_loop_inside_a_loop_starts_over_each_outer_pass() {
        let mut x = g("nested");
        let outer = x.add(NodeKind::ForEach, 0, 0);
        x.node_mut(outer).unwrap().config = json!({ "items": "a,b" });
        let inner = x.add(NodeKind::ForEach, 200, 0);
        x.node_mut(inner).unwrap().config = json!({ "items": "x,y" });
        let body = x.add(NodeKind::Print, 400, 0);
        x.node_mut(body).unwrap().config =
            json!({ "message": "cell", "level": "info", "on_screen": false });
        x.wire(outer, "loop_body", inner, "exec_in").unwrap();
        x.wire(inner, "loop_body", body, "exec_in").unwrap();
        agree("nested", &x);
    }

    #[test]
    fn a_straight_chain() {
        let mut x = g("chain");
        let a = x.add(NodeKind::Print, 0, 0);
        x.node_mut(a).unwrap().config =
            json!({ "message": "one", "level": "info", "on_screen": false });
        let b = x.add(NodeKind::Print, 200, 0);
        x.node_mut(b).unwrap().config =
            json!({ "message": "two", "level": "info", "on_screen": false });
        x.wire(a, "exec_out", b, "exec_in").unwrap();
        agree("chain", &x);
    }

    #[test]
    fn a_branch_leaves_the_other_arm_alone() {
        let mut x = g("branch");
        let gate = x.add(NodeKind::If, 0, 0);
        x.node_mut(gate).unwrap().config = json!({ "condition": true });
        let hot = x.add(NodeKind::Print, 200, -80);
        x.node_mut(hot).unwrap().config =
            json!({ "message": "hot", "level": "info", "on_screen": false });
        let cold = x.add(NodeKind::Print, 200, 80);
        x.node_mut(cold).unwrap().config =
            json!({ "message": "cold", "level": "info", "on_screen": false });
        x.wire(gate, "true", hot, "exec_in").unwrap();
        x.wire(gate, "false", cold, "exec_in").unwrap();
        agree("branch", &x);
    }

    /// A node with DURABLE state, driven by the scheduler here and cooperating with the engine.
    ///
    /// The interesting half is not that both fire — it is that both fire ONCE. A cooldown whose
    /// stamp the two schedulers wrote differently would show up as one of them opening a second
    /// time, and that is exactly the kind of difference nobody notices until a person gets forty
    /// notifications.
    #[test]
    fn a_cooldown_opens_once() {
        let mut x = g("cooldown");
        let gate = x.add(NodeKind::Cooldown, 0, 0);
        x.node_mut(gate).unwrap().config = json!({ "window_secs": "60" });
        let after = x.add(NodeKind::Print, 200, 0);
        x.node_mut(after).unwrap().config =
            json!({ "message": "through", "level": "info", "on_screen": false });
        x.wire(gate, "exec_out", after, "exec_in").unwrap();
        agree("cooldown", &x);
    }

    /// Alternating state, same question.
    #[test]
    fn a_flip_flop_takes_one_side() {
        let mut x = g("flip flop");
        let ff = x.add(NodeKind::FlipFlop, 0, 0);
        let a = x.add(NodeKind::Print, 200, -60);
        x.node_mut(a).unwrap().config =
            json!({ "message": "a", "level": "info", "on_screen": false });
        let b = x.add(NodeKind::Print, 200, 60);
        x.node_mut(b).unwrap().config =
            json!({ "message": "b", "level": "info", "on_screen": false });
        x.wire(ff, "a", a, "exec_in").unwrap();
        x.wire(ff, "b", b, "exec_in").unwrap();
        agree("flip flop", &x);
    }

    /// A display sink: writes durable state and pushes to the editor.
    ///
    /// Both schedulers must run it in the same place. The side effects are this product's own —
    /// the node reaches `ui_tx` directly rather than through an engine trait — so what is
    /// compared here is position, which is the thing that could differ.
    #[test]
    fn a_display_sink_runs_where_it_is_reached() {
        let mut x = g("display");
        let src = x.add(NodeKind::Format, 0, 0);
        x.node_mut(src).unwrap().config = json!({ "template": "42" });
        // A `Text` sink, not a `Gauge`: gauge's `value` is typed `num` and Format produces
        // `text`, so the wire is refused — which the port types are doing their job by refusing.
        let sink = x.add(NodeKind::Text, 200, 0);
        x.wire(src, "text", sink, "value").unwrap();
        agree("display", &x);
    }

    #[test]
    fn a_loop_runs_its_body_once_per_item() {
        let mut x = g("loop");
        let each = x.add(NodeKind::ForEach, 0, 0);
        x.node_mut(each).unwrap().config = json!({ "items": "a,b,c" });
        let body = x.add(NodeKind::Print, 200, 0);
        x.node_mut(body).unwrap().config =
            json!({ "message": "item", "level": "info", "on_screen": false });
        x.wire(each, "loop_body", body, "exec_in").unwrap();
        agree("loop", &x);
    }

    /// A pure source on each arm of a branch — the shape of #115 and #205.
    ///
    /// Both sources activate: that is deliberate and load-bearing, because a sub-run entering
    /// halfway still has to find its variables seeded (#114). Only one of them was ever read, and
    /// only that one may reach a person. Until the deferral moved out of the driver this shape
    /// could not be compared at all: the driver reported the surviving source and the engine
    /// reported both, so any graph with a `GetVariable` in it was outside the oracle.
    #[test]
    fn a_source_on_a_dead_arm_reaches_nobody() {
        let mut gr = g("source on a dead arm");
        let gate = gr.add(NodeKind::If, 0, 0);
        gr.node_mut(gate).unwrap().config["condition"] = json!(false);

        let get_dead = gr.add(NodeKind::GetVariable, 0, 0);
        gr.node_mut(get_dead).unwrap().config["variable"] = json!("a");
        gr.node_mut(get_dead).unwrap().config["type"] = json!("text");
        let fmt_dead = gr.add(NodeKind::Format, 0, 0);
        gr.node_mut(fmt_dead).unwrap().config["template"] = json!("{p1}");
        gr.wire(gate, "true", fmt_dead, "exec_in").unwrap();
        gr.wire(get_dead, "value", fmt_dead, "p1").unwrap();

        let get_live = gr.add(NodeKind::GetVariable, 0, 0);
        gr.node_mut(get_live).unwrap().config["variable"] = json!("b");
        gr.node_mut(get_live).unwrap().config["type"] = json!("text");
        let fmt_live = gr.add(NodeKind::Format, 0, 0);
        gr.node_mut(fmt_live).unwrap().config["template"] = json!("{p1}");
        gr.wire(gate, "false", fmt_live, "exec_in").unwrap();
        gr.wire(get_live, "value", fmt_live, "p1").unwrap();

        agree("source on a dead arm", &gr);

        // And say out loud what "agree" alone would not: that they agree on the RIGHT answer, not
        // merely on the same one.
        let seen = order_engine(&gr, test_ctx()).expect("the engine runs it");
        assert!(
            !seen.contains(&get_dead),
            "the source on the arm nobody took must not reach a person: {seen:?}"
        );
        assert!(
            seen.contains(&get_live),
            "the source that was read must: {seen:?}"
        );
    }
}
