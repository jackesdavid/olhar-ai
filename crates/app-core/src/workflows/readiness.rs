//! Is this graph finished — wired up, with every required input filled?
//!
//! The question a list of workflows asks about each row, and the reason the list used to ship every
//! node and every edge of every graph: the badge could not be drawn without the whole document.
//!
//! # This is the engine's job, held here
//!
//! `ggraph_core::readiness` answers exactly this, and Redoma already calls it. This crate is pinned
//! to an engine tag that predates it. **Delete this module when that pin moves** and call the engine
//! — the rule belongs in one place, and two copies is how a list comes to say "ready" about a graph
//! that then refuses to run.
//!
//! The rule is deliberately the one the editor has always applied: wired, or a non-empty entry under
//! the port's own name. Moving it here changes what travels, not what a badge says.

use super::domain::Graph;
use super::registry::registry;
use ggraph_core::PortType;
use serde_json::Value as Json;

/// Does a required input have nothing on it?
fn empty(v: Option<&Json>) -> bool {
    match v {
        None | Some(Json::Null) => true,
        Some(Json::String(s)) => s.trim().is_empty(),
        Some(Json::Object(o)) => o.is_empty(),
        Some(Json::Array(a)) => a.is_empty(),
        _ => false,
    }
}

/// Is any required input neither wired nor filled in the inspector?
///
/// A node of a kind this build does not register is skipped: that is a different problem, with a
/// different message, and listing every port of it here would bury it.
pub fn has_unfilled(g: &Graph) -> bool {
    let reg = registry();
    g.nodes.iter().any(|n| {
        let Some(spec) = reg.get(&n.kind) else {
            return false;
        };
        spec.inputs.resolve(&n.config).into_iter().any(|p| {
            p.required
                && p.ty != PortType::EXEC
                && !g
                    .edges
                    .iter()
                    .any(|e| e.to == n.id && e.to_port == p.name)
                && empty(n.config.get(p.name.as_str()))
        })
    })
}

/// Does the document describe anything to run? One wired node is the bar; with no edges every node
/// is an orphan, and whether orphans run is the graph's own setting.
pub fn runnable(g: &Graph) -> bool {
    g.meta.run_orphans || !g.edges.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn graph(nodes: Json, edges: Json) -> Graph {
        serde_json::from_value(json!({
            "id": "00000000-0000-0000-0000-000000000000",
            "name": "t", "nodes": nodes, "edges": edges
        }))
        .expect("a graph")
    }

    #[test]
    fn a_required_port_with_nothing_on_it_is_unfilled() {
        let g = graph(json!([{ "id": 1, "kind": "create_alert", "x": 0, "y": 0 }]), json!([]));
        assert!(has_unfilled(&g));
    }

    #[test]
    fn a_value_in_the_inspector_fills_it() {
        let g = graph(
            json!([{ "id": 1, "kind": "create_alert", "x": 0, "y": 0,
                     "config": { "message": "hello" } }]),
            json!([]),
        );
        assert!(!has_unfilled(&g));
    }

    /// A field somebody clicked into and left is empty, whatever whitespace it holds.
    #[test]
    fn a_blank_field_does_not_fill_it() {
        let g = graph(
            json!([{ "id": 1, "kind": "create_alert", "x": 0, "y": 0, "config": { "message": "  " } }]),
            json!([]),
        );
        assert!(has_unfilled(&g));
    }

    /// With no wires every node is an orphan and a run is a no-op — which is why Run is disabled.
    #[test]
    fn a_document_with_no_wires_is_not_runnable() {
        let g = graph(json!([{ "id": 1, "kind": "print", "x": 0, "y": 0 }]), json!([]));
        assert!(!runnable(&g));
    }
}
