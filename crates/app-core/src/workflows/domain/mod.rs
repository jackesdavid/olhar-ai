//! Workflows domain — typed ports, the node catalog, graph documents, persistence.
//!
//! Split by what each part answers, because it was one file of three thousand lines and the
//! sections did not talk to each other much:
//!
//! - [`ports`] — what travels along a wire, and the shapes behind each type;
//! - [`kind_view`] — how this product's `NodeKind` enum and the engine's open string identifier
//!   are the same thing seen from two sides;
//! - [`kinds`] — the catalog itself: every node kind, and what it is called;
//! - [`shape`] — what each kind looks like: its pins, its arms, its starting configuration;
//! - [`elements`] — what a node's ports actually are once configured, plus the run policies;
//! - [`graph`] — the stored document and the editing surface that refuses a bad wire;
//! - [`store`] — a named collection of graphs.
//!
//! Everything is re-exported here, so `domain::Anything` still resolves exactly as it did when
//! this was one file. The split is for whoever has to read it, not for callers.
//!
//! Isolation model, unchanged: a node declares typed input/output **ports**; **edges** map a
//! source output port to a target input port. The engine owns all edges and resolves wiring — a
//! node never references another node.

pub use crate::events::AlertLevel;

pub mod elements;
pub mod graph;
pub mod kind_view;
pub mod kinds;
pub mod ports;
pub mod shape;
pub mod store;

pub use elements::*;
pub use graph::*;
pub use kind_view::*;
pub use kinds::*;
pub use ports::*;
pub use store::*;

#[cfg(test)]
mod instance_scope_tests {
    use super::*;
    use crate::workflows::{PortValue, PortValues};

    fn payload_with_camera(v: PortValue) -> PortValues {
        let mut p = PortValues::new();
        p.insert("camera".into(), v);
        p
    }

    #[test]
    fn single_is_one_instance_and_ignores_the_camera() {
        // The default, and the migration's safety: every graph alive today is Single, keys on "",
        // and addresses exactly the state rows it always did.
        let s = InstanceScope::Single;
        assert_eq!(s.key_for(&PortValues::new()), "");
        assert_eq!(
            s.key_for(&payload_with_camera(PortValue::Text("10.0.0.9".into()))),
            ""
        );
        assert_eq!(InstanceScope::default(), InstanceScope::Single);
    }

    #[test]
    fn per_camera_separates_cameras() {
        // The whole point: A and B are different instances, so they can't share a window.
        let s = InstanceScope::PerCamera;
        let a = s.key_for(&payload_with_camera(PortValue::Text("cam-a".into())));
        let b = s.key_for(&payload_with_camera(PortValue::Text("cam-b".into())));
        assert_ne!(a, b);
        assert_eq!(a, "cam-a");
    }

    #[test]
    fn per_camera_is_stable_for_the_same_camera() {
        // And the other half: the debounce only survives if a re-trigger lands on the SAME key.
        let s = InstanceScope::PerCamera;
        let first = s.key_for(&payload_with_camera(PortValue::Text("cam-a".into())));
        let again = s.key_for(&payload_with_camera(PortValue::Text("cam-a".into())));
        assert_eq!(
            first, again,
            "a re-trigger must find the same instance or the debounce dies"
        );
    }

    #[test]
    fn per_camera_accepts_the_camera_either_side_of_seeding() {
        // The wire carries Text (ip/id); `seed_event_payload` upgrades it to a CameraEntity. A
        // DelayTrigger can arm on either side of that, and both must key the same instance.
        let s = InstanceScope::PerCamera;
        let raw = s.key_for(&payload_with_camera(PortValue::Text("cam-1".into())));
        let typed = s.key_for(&payload_with_camera(PortValue::Camera(CameraEntity {
            id: "cam-1".into(),
            ip: "10.0.0.9".into(),
            name: "Hall".into(),
        })));
        assert_eq!(
            raw, typed,
            "Text and CameraEntity must resolve to one instance"
        );
    }

    #[test]
    fn per_camera_without_a_camera_is_one_honest_instance() {
        // An event with no camera (telegram_message, say) in a per-camera graph: one instance,
        // not a broken one. "" is a real key, not a missing value.
        assert_eq!(InstanceScope::PerCamera.key_for(&PortValues::new()), "");
    }

    #[test]
    fn scope_survives_json_and_old_graphs_have_none() {
        let g: Graph =
            serde_json::from_str(r#"{"id":"00000000-0000-0000-0000-000000000000","name":"old"}"#)
                .unwrap();
        assert_eq!(
            g.meta.instance_scope,
            InstanceScope::Single,
            "a graph saved before instances is Single"
        );
        let mut g2 = Graph::new("g");
        g2.meta.instance_scope = InstanceScope::PerCamera;
        let back: Graph = serde_json::from_str(&serde_json::to_string(&g2).unwrap()).unwrap();
        assert_eq!(back.meta.instance_scope, InstanceScope::PerCamera);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn chain() -> (Graph, u32, u32) {
        // get_last_alert.message -> create_alert.message
        let mut g = Graph::new("test");
        let a = g.add(NodeKind::GetLastAlert, 0, 0);
        let b = g.add(NodeKind::CreateAlert, 30, 0);
        g.wire(a, "message", b, "message").unwrap();
        (g, a, b)
    }

    #[test]
    fn add_node_assigns_incrementing_ids_and_default_config() {
        let mut g = Graph::new("g");
        let a = g.add(NodeKind::CreateAlert, 1, 2);
        let b = g.add(NodeKind::Format, 3, 4);
        assert_eq!((a, b), (1, 2));
        assert_eq!(g.next_id, 3);
        assert_eq!(g.node(a).unwrap().config["source"], "workflow");
    }

    #[test]
    fn remove_node_cascades_edges() {
        let (mut g, a, b) = chain();
        assert_eq!(g.edges.len(), 1);
        g.remove_node(a);
        assert!(g.node(a).is_none());
        assert_eq!(g.edges.len(), 0);
        assert_eq!(g.children(b), Vec::<u32>::new());
    }

    #[test]
    fn children_and_topo_order_follow_edges() {
        let (g, a, b) = chain();
        assert_eq!(g.children(a), vec![b]);
        assert_eq!(g.topo_order().unwrap(), vec![a, b]);
        assert_eq!(g.roots(), vec![a]);
    }

    #[test]
    fn add_edge_rejects_type_mismatch_double_fed_cycle_and_unknown() {
        let mut g = Graph::new("g");
        let snap = g.add(NodeKind::CameraSnapshot, 0, 0);
        let fmt = g.add(NodeKind::Format, 0, 0);
        assert!(g
            .wire(snap, "image", fmt, "p1")
            .unwrap_err()
            .contains("cannot wire"));
        assert!(g.wire(fmt, "text", fmt, "p1").is_err()); // self-loop
        assert!(g.wire(snap, "nope", fmt, "p1").is_err()); // unknown port

        let gla = g.add(NodeKind::GetLastAlert, 0, 0);
        let ca = g.add(NodeKind::CreateAlert, 0, 0);
        g.wire(gla, "message", ca, "message").unwrap();
        assert!(g
            .wire(fmt, "text", ca, "message")
            .unwrap_err()
            .contains("already wired"));

        let a = g.add(NodeKind::Format, 0, 0);
        let b = g.add(NodeKind::Format, 0, 0);
        g.wire(a, "text", b, "p1").unwrap();
        assert!(g.wire(b, "text", a, "p1").unwrap_err().contains("cycle"));
    }

    #[test]
    fn exec_ports_wire_between_any_nodes_but_not_to_data() {
        let mut g = Graph::new("g");
        let a = g.add(NodeKind::GetLastAlert, 0, 0);
        let w = g.add(NodeKind::Wait, 0, 0);
        let b = g.add(NodeKind::CreateAlert, 0, 0);
        // exec → exec sequences nodes that share no data.
        g.wire(a, "exec_out", w, "exec_in").unwrap();
        g.wire(w, "exec_out", b, "exec_in").unwrap();
        // exec can't connect to a data port.
        assert!(g
            .wire(a, "exec_out", b, "message")
            .unwrap_err()
            .contains("cannot wire"));
        assert!(g
            .wire(a, "message", b, "exec_in")
            .unwrap_err()
            .contains("cannot wire"));
        // Wait is control-flow only — it has no data ports.
        assert!(NodeKind::Wait.input_ports().is_empty());
        assert!(NodeKind::Wait.output_ports().is_empty());
    }

    #[test]
    fn if_node_has_true_false_exec_outputs() {
        let mut g = Graph::new("g");
        let gate = g.add(NodeKind::If, 0, 0);
        let a = g.add(NodeKind::Format, 0, 0);
        // Both branch pins are wireable, typed Exec.
        g.wire(gate, "true", a, "exec_in").unwrap();
        let b = g.add(NodeKind::Format, 0, 0);
        g.wire(gate, "false", b, "exec_in").unwrap();
        let names: Vec<_> = NodeKind::If
            .exec_out_ports()
            .iter()
            .map(|p| p.name.clone())
            .collect();
        assert_eq!(names, vec!["true", "false"]);
        assert_eq!(NodeKind::If.output_port("true").unwrap().ty, types::EXEC);
        // `condition` is a required Bool data input.
        assert!(NodeKind::If.input_port("condition").unwrap().required);
    }

    #[test]
    fn graph_json_round_trips_with_trigger_concurrency_and_typed_edges() {
        let (mut g, _a, _b) = chain();
        g.meta.trigger = Trigger::Manual;
        g.meta.concurrency = ConcurrencyPolicy::QueueSingle { max_queued: 3 };
        g.meta.enabled = true;
        let s = serde_json::to_string(&g).unwrap();
        let g2: Graph = serde_json::from_str(&s).unwrap();
        assert_eq!(g, g2);
    }

    #[test]
    fn workflows_collection_create_remove_lookup() {
        let mut w = Workflows::from_graphs(Vec::new());
        let id1 = w.create_graph("alpha");
        let id2 = w.create_graph("beta");
        assert_ne!(id1, id2);
        assert_eq!(w.graphs.len(), 2);
        assert_eq!(w.graph(id2).unwrap().name, "beta");
        w.remove_graph(id1);
        assert_eq!(w.graphs.len(), 1);
        assert!(w.graph(id1).is_none());
    }

    #[test]
    fn slug_round_trips_all_kinds() {
        for k in NodeKind::ALL {
            assert_eq!(NodeKind::from_slug(k.slug()), Some(k));
        }
    }

    #[test]
    fn string_switch_wires_arm_and_default_exec_ports() {
        let mut g = Graph::new("g");
        let sw = g.add(NodeKind::StringSwitch, 0, 0);
        g.node_mut(sw).unwrap().config = json!({ "arms": ["red", "blue"] });
        let a = g.add(NodeKind::Format, 0, 0);
        let b = g.add(NodeKind::Format, 0, 0);
        let d = g.add(NodeKind::Format, 0, 0);
        // Named arm ports resolve and wire correctly.
        g.wire(sw, "red", a, "exec_in").expect("'red' arm wires");
        g.wire(sw, "blue", b, "exec_in").expect("'blue' arm wires");
        // The built-in default arm also wires.
        g.wire(sw, "default", d, "exec_in")
            .expect("'default' arm wires");
        // An arm not in config is rejected.
        let c = g.add(NodeKind::Format, 0, 0);
        let err = g.wire(sw, "green", c, "exec_in").unwrap_err();
        assert!(err.contains("has no port"), "unknown arm rejected: {err}");
    }

    #[test]
    fn string_switch_value_port_is_required_text() {
        let k = NodeKind::StringSwitch;
        let p = k.input_port("value").expect("value port exists");
        assert_eq!(p.ty, types::TEXT);
        assert!(p.required);
        // exec_out_ports is empty (dynamic); output_ports is empty.
        assert!(k.exec_out_ports().is_empty());
        assert!(k.output_ports().is_empty());
    }

    #[test]
    fn string_switch_default_config_has_two_arms() {
        let cfg = NodeKind::StringSwitch.default_config();
        let arms = cfg["arms"].as_array().expect("arms is an array");
        assert_eq!(arms.len(), 2);
        assert_eq!(arms[0].as_str(), Some("arm_1"));
        assert_eq!(arms[1].as_str(), Some("arm_2"));
    }

    /// The multi variable nodes derive a port per configured entry, named after the variable and
    /// typed by its declaration — the whole point of the pair (#179).
    #[test]
    fn multi_variable_nodes_derive_one_port_per_variable() {
        let mut g = Graph::new("multi");
        let set = g.add(NodeKind::MultiSetVariable, 0, 0);
        let get = g.add(NodeKind::MultiGetVariable, 0, 0);
        let cfg = json!({ "variables": [
            { "name": "bbox", "type": "bbox" },
            { "name": "confidence", "type": "num" },
        ] });
        g.node_mut(set).unwrap().config = cfg.clone();
        g.node_mut(get).unwrap().config = cfg;

        let ins = g.node(set).unwrap().input_ports();
        assert_eq!(ins.len(), 2, "Set grows one input per variable");
        assert_eq!(ins[0].name, "bbox");
        assert_eq!(ins[0].ty, types::BBOX);
        assert_eq!(ins[1].ty, types::NUM);
        // Never required: an unwired input means "leave this variable alone".
        assert!(ins.iter().all(|p| !p.required));
        // Set writes; it exposes no data outputs, only its exec passthrough.
        assert!(g.node(set).unwrap().output_ports().is_empty());

        let outs = g.node(get).unwrap().output_ports();
        assert_eq!(outs.len(), 2, "Get grows one output per variable");
        assert_eq!(outs[1].name, "confidence");
        assert!(g.node(get).unwrap().input_ports().is_empty());
        // Pure, like the single Get — drop it anywhere and wire it, no flow wires needed.
        assert!(NodeKind::MultiGetVariable.is_pure());
        assert!(!NodeKind::MultiSetVariable.is_pure());
    }

    /// Blank and duplicate names are dropped rather than emitted: an unnamed port cannot be wired
    /// to, and two ports sharing a name would collapse in the `PortValues` map, so the second
    /// would silently win at run time while the canvas showed both.
    #[test]
    fn multi_variable_ports_drop_blanks_and_duplicates() {
        let ports = multi_variable_ports(&json!({ "variables": [
            { "name": "a", "type": "num" },
            { "name": "  ", "type": "num" },
            { "name": "a", "type": "text" },
            { "name": " b ", "type": "bool" },
        ] }));
        let names: Vec<&str> = ports.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, vec!["a", "b"], "blanks and repeats are dropped");
        assert_eq!(ports[0].ty, types::NUM, "the FIRST entry wins");
        assert_eq!(ports[1].name, "b", "names are trimmed");
    }

    /// An empty list is a node with no ports at all — not a crash, and not a default port. It is
    /// the state a freshly dropped node is in.
    #[test]
    fn multi_variable_ports_of_a_fresh_node_are_empty() {
        let cfg = NodeKind::MultiSetVariable.default_config();
        assert!(multi_variable_ports(&cfg).is_empty());
        assert!(multi_variable_ports(&json!({})).is_empty());
    }
}
