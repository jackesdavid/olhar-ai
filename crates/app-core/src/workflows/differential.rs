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
//! It is not a claim that the schedulers agree in general. It is a growing corpus, and the
//! honest reading of a passing run is "these shapes agree", nothing wider. Shapes are added as
//! they are understood; [`KNOWN_DIFFERENCES`] lists the ones that do not agree yet and why,
//! rather than letting them sit as a quietly ignored failure.

use super::domain::Graph;
use super::host::{SentinelHost, SentinelMeta};
use super::node::PortValues;
use crate::agent::tools::ToolContext;
use ggraph_core::host::{Observer, UiEvent};
use ggraph_core::{Entry, RunOptions};
use std::sync::{Arc, Mutex};

/// Shapes that do not agree yet, and why. A shape listed here is skipped by the sweep rather
/// than silently failing it — a red test nobody can fix is a red test everybody learns to ignore.
const KNOWN_DIFFERENCES: &[(&str, &str)] = &[(
    "loop",
    "the engine runs the body once; this product runs it once per item. `ForEach` is driven \
         by the scheduler here — it is one of the 19 inline `n.kind ==` blocks — and the adapter \
         cannot drive it, because a Run node cannot ask to be re-entered. Fixed by giving the \
         cooperating kinds a real `NodeStep`.",
)];

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
    let host = SentinelHost::watching(sys, Box::new(RecRef(rec.clone())));
    let doc = to_engine_graph(g);
    ggraph_core::run(
        &doc,
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

/// This product's `Graph` as the engine's document.
///
/// A conversion, not a cast, because this product's `Graph` still owns its own struct. When it
/// becomes `Graph<SentinelMeta>` this function disappears — and the fact that it is this short
/// is the evidence that it can.
fn to_engine_graph(g: &Graph) -> ggraph_core::Graph<SentinelMeta> {
    let mut doc: ggraph_core::Graph<SentinelMeta> = ggraph_core::Graph::new(&g.name);
    doc.id = g.id;
    doc.next_id = g.next_id;
    doc.meta = SentinelMeta {
        enabled: g.enabled,
        trigger: g.trigger.clone(),
        concurrency: g.concurrency.clone(),
        instance_scope: g.instance_scope.clone(),
        run_orphans: g.run_orphans,
        debug: g.debug,
    };
    doc.nodes = g
        .nodes
        .iter()
        .map(|n| ggraph_core::GraphNode {
            id: n.id,
            kind: ggraph_core::NodeId::new(n.kind.slug()),
            x: n.x,
            y: n.y,
            config: n.config.clone(),
        })
        .collect();
    doc.edges = g.edges.clone();
    doc
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
        g.enabled = true;
        g
    }

    #[test]
    fn a_straight_chain() {
        let mut x = g("chain");
        let a = x.add_node(NodeKind::Print, 0, 0);
        x.node_mut(a).unwrap().config =
            json!({ "message": "one", "level": "info", "on_screen": false });
        let b = x.add_node(NodeKind::Print, 200, 0);
        x.node_mut(b).unwrap().config =
            json!({ "message": "two", "level": "info", "on_screen": false });
        x.add_edge(a, "exec_out", b, "exec_in").unwrap();
        agree("chain", &x);
    }

    #[test]
    fn a_branch_leaves_the_other_arm_alone() {
        let mut x = g("branch");
        let gate = x.add_node(NodeKind::If, 0, 0);
        x.node_mut(gate).unwrap().config = json!({ "condition": true });
        let hot = x.add_node(NodeKind::Print, 200, -80);
        x.node_mut(hot).unwrap().config =
            json!({ "message": "hot", "level": "info", "on_screen": false });
        let cold = x.add_node(NodeKind::Print, 200, 80);
        x.node_mut(cold).unwrap().config =
            json!({ "message": "cold", "level": "info", "on_screen": false });
        x.add_edge(gate, "true", hot, "exec_in").unwrap();
        x.add_edge(gate, "false", cold, "exec_in").unwrap();
        agree("branch", &x);
    }

    #[test]
    fn a_loop_runs_its_body_once_per_item() {
        let mut x = g("loop");
        let each = x.add_node(NodeKind::ForEach, 0, 0);
        x.node_mut(each).unwrap().config = json!({ "items": "a,b,c" });
        let body = x.add_node(NodeKind::Print, 200, 0);
        x.node_mut(body).unwrap().config =
            json!({ "message": "item", "level": "info", "on_screen": false });
        x.add_edge(each, "loop_body", body, "exec_in").unwrap();
        agree("loop", &x);
    }
}
