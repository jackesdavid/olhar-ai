//! Sentinel's node set, as the engine sees it.
//!
//! Ninety-one kinds, registered once at first use. The engine's registry is the source of truth
//! for what a node *declares* — its ports, its category, its default config, its deadline — and
//! everything that used to read those from twelve separate `match` arms now reads them here.
//!
//! ## Built from the enum rather than replacing it
//!
//! The specs are derived from `NodeKind` by iterating it, not by hand-writing ninety-one
//! declarations. That is deliberate and it is not laziness: the port tables, labels, categories
//! and defaults already exist and are already correct, and restating them by hand would be a
//! transcription with ninety-one chances to introduce a difference the catalog snapshot would
//! then dutifully freeze.
//!
//! `NodeKind` becomes an implementation detail of this file. Splitting the kinds into one file
//! each is a later, purely mechanical move, and one that can be done a family at a time with the
//! catalog proving each step.

use super::domain::{self, NodeKind};
use super::host::SentinelHost;
use super::node;
use ggraph_core::spec::{
    ExecOut, NodeCx, NodeError, NodeRoute, NodeRun, NodeSpec, Ports, Purity, Timeout,
};
use ggraph_core::{NodeRegistry, PortValues};
use serde_json::Value as Json;
use std::sync::OnceLock;

/// The process-wide node set.
pub fn registry() -> &'static NodeRegistry<SentinelHost> {
    static R: OnceLock<NodeRegistry<SentinelHost>> = OnceLock::new();
    R.get_or_init(build)
}

fn build() -> NodeRegistry<SentinelHost> {
    let mut reg = NodeRegistry::new();
    for k in NodeKind::ALL {
        reg.register(spec_for(k));
    }
    // `ALL` is the palette, not the node set. Reroute is a real kind that a stored graph can
    // name and that nobody should add from the palette, so it is registered and hidden. Seeding
    // a registry from the palette list instead is how a saved graph containing a wire organiser
    // stops opening.
    reg.register(spec_for(NodeKind::Reroute).hidden());
    reg
}

/// Executes a kind through the existing per-kind implementation.
///
/// One adapter for all ninety-one rather than ninety-one structs. The bodies stay where they
/// are, in `node/*.rs`, until the scheduler swap makes it worth moving them — at which point
/// each becomes a `NodeRun` in its own file and this adapter disappears.
struct Legacy(NodeKind);

impl NodeRoute<SentinelHost> for Legacy {
    fn arms(&self, cx: &NodeCx<'_, SentinelHost>, out: &PortValues) -> Vec<ggraph_core::PortName> {
        let inputs = super::compat::from_engine_values(cx.inputs);
        let outputs = super::compat::from_engine_values(out);
        node::arms_for(self.0, cx.config, &inputs, &outputs)
            .into_iter()
            .map(ggraph_core::PortName::new)
            .collect()
    }
}

impl NodeRun<SentinelHost> for Legacy {
    fn run(&self, cx: &NodeCx<'_, SentinelHost>) -> Result<PortValues, NodeError> {
        let inputs = super::compat::from_engine_values(cx.inputs);
        let out =
            node::run_node(self.0, cx.config, &inputs, cx.host.tools()).map_err(NodeError::new)?;
        Ok(super::compat::to_engine_values(&out))
    }

    /// The line a person reads in the Console.
    ///
    /// Without this the engine's default reports the raw outputs, which is a reasonable thing for
    /// an engine to say and useless next to what each kind knows to say about itself: which two
    /// numbers were compared and which way it went, which camera a frame came from, how many rows
    /// a query matched. `debug_summary` has one arm per kind for exactly that, and leaving it
    /// unwired would have quietly emptied the Console of everything worth reading while still
    /// printing a line for every node.
    fn summary(&self, cx: &NodeCx<'_, SentinelHost>, out: &PortValues) -> String {
        node::debug_summary(
            self.0,
            cx.config,
            &super::compat::from_engine_values(cx.inputs),
            &super::compat::from_engine_values(out),
        )
    }
}

fn spec_for(k: NodeKind) -> NodeSpec<SentinelHost> {
    // The order matters, and the reason is a trap worth naming: this product's
    // `is_pure_source` means "run this even when it is an orphan entry inside a sub-run" — a
    // SEEDING rule — while the engine's `PureSource` means "re-evaluate on every read" — a
    // CACHING rule. Same name, different concept, and several kinds are the first without being
    // the second. `get_recipient` is one: a pure source by this product's meaning, and yet it
    // has exec pins. Deciding purity from `is_pure_source` first silently stripped its exec pin
    // out of the catalog, which the snapshot caught.
    let purity = if !k.is_pure() {
        Purity::EFFECTFUL
    } else if k.is_pure_source() {
        Purity::PURE_SOURCE
    } else {
        Purity::PURE
    };

    let base = if purity.has_exec() {
        NodeSpec::effectful(k.slug(), k.label(), k.category())
    } else {
        NodeSpec::pure(k.slug(), k.label(), k.category())
    };

    let declared = base
        .with_purity(purity)
        .with_inputs(Ports::dynamic(move |cfg: &Json| {
            domain::resolved_input_ports(k, cfg)
        }))
        .with_outputs(Ports::dynamic(move |cfg: &Json| {
            domain::resolved_output_ports(k, cfg)
        }))
        .with_exec_out(exec_out_for(k, purity))
        .with_config(move || k.default_config())
        .with_timeout(timeout_for(k))
        .with_aliases(aliases_for(k));

    // A routing kind CHOOSES an arm; everything else fires the arms it declares. Registering
    // them all as `Run` was the first thing the differential test caught — a Run node fires
    // every declared arm, so `if` fired `true` and `false` together and the engine ran both
    // branches of every graph.
    // A kind the scheduler DRIVES gets a real cooperating behaviour; the rest go through the
    // adapter. Each one moved here turns a `KNOWN_DIFFERENCES` entry in the differential test
    // into an agreement, which is how the scheduler swap is being paid for incrementally.
    // A comment is a label on the canvas. It has no behaviour, and saying so in the type is
    // better than a scheduler branch that skips it: `Inert` is what the engine calls a node that
    // is real, wireable and does nothing.
    if k == NodeKind::Comment {
        return declared;
    }
    if k == NodeKind::ForEach {
        return declared.stepping(super::steps::for_each::ForEach);
    }
    if k == NodeKind::Cooldown {
        return declared.stepping(super::steps::cooldown::Cooldown);
    }
    if k == NodeKind::FlipFlop {
        return declared.stepping(super::steps::flip_flop::FlipFlop);
    }
    if k == NodeKind::DeviceGate {
        return declared.stepping(super::steps::device_gate::DeviceGate);
    }
    if k == NodeKind::DelayTrigger {
        return declared.stepping(super::steps::delay_trigger::DelayTrigger);
    }
    if k == NodeKind::OnEvent {
        return declared.stepping(super::steps::on_event::OnEvent);
    }
    if k == NodeKind::EmitEvent {
        return declared.stepping(super::steps::emit_event::EmitEvent);
    }
    if k == NodeKind::ShowImage {
        return declared.stepping(super::steps::show_image::ShowImage);
    }
    if k == NodeKind::Text || k == NodeKind::Gauge {
        return declared.stepping(super::steps::display::Display(k));
    }
    if node::is_routing(k) {
        declared.routing(Legacy(k))
    } else {
        declared.running(Legacy(k))
    }
}

fn exec_out_for(k: NodeKind, purity: Purity) -> ExecOut {
    if !purity.has_exec() {
        return ExecOut::None;
    }
    match k {
        // The three switches name their arms from configuration. `camera_switch` numbers them
        // positionally while the other two use the label, and the wire format depends on both.
        NodeKind::StringSwitch | NodeKind::AiSwitch | NodeKind::CameraSwitch => {
            ExecOut::dynamic(move |cfg: &Json| domain::resolved_exec_out_ports(k, cfg))
        }
        _ => ExecOut::Static(k.exec_out_ports()),
    }
}

fn timeout_for(k: NodeKind) -> Timeout {
    // Asked with the node's REAL configuration, at the moment it runs.
    //
    // This used to hand `node_timeout` an empty object, because a spec could only declare a
    // constant. That threw away the per-node `timeout_secs` a person had typed — so a slow
    // endpoint set to wait sixty seconds got the kind's default instead, and failed on exactly
    // the requests it had been configured to wait for.
    Timeout::from_config(move |cfg: &Json| node::node_timeout(k, cfg).map(|d| d.as_secs().max(1)))
}

fn aliases_for(k: NodeKind) -> &'static [&'static str] {
    match k {
        // Renamed when beacons became recipients. Every graph saved before that still names the
        // old slug, and a rename that does not carry its alias is those graphs failing to load —
        // at load time, in front of somebody.
        NodeKind::NotifyRecipient => &["notify_beacon"],
        _ => &[],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ggraph_core::spec::Behavior;

    #[test]
    fn every_kind_is_registered_including_the_hidden_one() {
        let reg = registry();
        assert_eq!(
            reg.len(),
            91,
            "the enum has 91 kinds; ALL lists 90 because Reroute is not in the palette"
        );
        assert_eq!(reg.palette().count(), 90);
        assert!(
            reg.resolve("reroute").is_some(),
            "a stored graph naming a wire organiser must still load"
        );
    }

    #[test]
    fn the_renamed_node_still_answers_to_its_old_name() {
        assert_eq!(
            registry().resolve("notify_beacon").map(|s| s.id.clone()),
            registry().resolve("notify_recipient").map(|s| s.id.clone())
        );
    }

    #[test]
    fn a_switch_declares_the_arms_its_configuration_names() {
        let spec = registry().resolve("string_switch").expect("registered");
        let cfg = serde_json::json!({ "arms": ["alpha", "beta"] });
        let arms: Vec<String> = spec
            .exec_out
            .resolve(&cfg)
            .iter()
            .map(|p| p.name.as_str().to_string())
            .collect();
        assert_eq!(
            arms,
            vec!["alpha", "beta", "default"],
            "wiring must see the arms the author configured, not a static table"
        );
    }

    #[test]
    fn config_derived_inputs_reach_the_registry() {
        let spec = registry().resolve("emit_event").expect("registered");
        let cfg = serde_json::json!({
            "payload": [{ "name": "plate", "type": "text" }]
        });
        let names: Vec<String> = spec
            .inputs
            .resolve(&cfg)
            .iter()
            .map(|p| p.name.as_str().to_string())
            .collect();
        assert!(
            names.contains(&"plate".to_string()),
            "an event's payload field must appear as a port: {names:?}"
        );
    }

    /// Every kind has something to run, except the ones that genuinely do nothing.
    ///
    /// `Behavior::Inert` is a claim, not a default: it says this node is real and wireable and
    /// has no behaviour. Exactly one kind qualifies — a comment is a label on the canvas. The
    /// list is pinned rather than the count, so a kind that becomes inert by accident during the
    /// scheduler swap fails here with its own name instead of passing quietly as a no-op.
    #[test]
    fn only_the_kinds_that_do_nothing_are_inert() {
        let inert: Vec<&str> = registry()
            .all()
            .filter(|s| matches!(s.behavior, Behavior::Inert))
            .map(|s| s.id.as_str())
            .collect();
        assert_eq!(
            inert,
            vec!["comment"],
            "a kind lost its behaviour, or gained an inertness nobody decided on"
        );
    }

    /// A node that CHOOSES an arm is registered as one, and everything else is not.
    ///
    /// The distinction is not decorative: a `Run` node fires every exec arm it declares, so an
    /// `if` registered as `Run` fires `true` and `false` together — which is what happened, and
    /// what the differential test caught.
    #[test]
    fn the_kinds_that_choose_an_arm_are_registered_as_routing() {
        let routing: Vec<&str> = registry()
            .all()
            .filter(|s| matches!(s.behavior, Behavior::Route(_)))
            .map(|s| s.id.as_str())
            .collect();
        let mut expected = vec![
            "if",
            "ask_ai",
            "string_switch",
            "ai_switch",
            "camera_switch",
        ];
        let mut got = routing.clone();
        expected.sort_unstable();
        got.sort_unstable();
        assert_eq!(got, expected, "the routing set drifted");
    }
}

#[cfg(test)]
mod timeout_tests {
    use super::*;

    /// A per-node `timeout_secs` reaches the scheduler.
    ///
    /// The registry used to declare each kind's timeout from an EMPTY configuration, because a
    /// spec could only hold a constant. Everything a person typed into a node was discarded on the
    /// way, and the symptom was a node failing on precisely the slow calls it had been given more
    /// time for.
    #[test]
    fn a_node_gets_the_time_its_own_configuration_asks_for() {
        let reg = registry();
        let spec = reg.resolve("http_post").expect("registered");

        let default = spec.timeout.resolve(&NodeKind::HttpPost.default_config());
        let generous = spec
            .timeout
            .resolve(&serde_json::json!({ "timeout_secs": "60" }));

        assert_eq!(generous, Some(60), "what the person typed is what it gets");
        assert_ne!(
            generous, default,
            "and it differs from the default, or this test proves nothing"
        );
    }

    /// A node that declared it cannot block still runs inline, whatever is in its configuration.
    #[test]
    fn a_pure_node_stays_inline() {
        let reg = registry();
        let spec = reg.resolve("num_gte").expect("registered");
        assert_eq!(
            spec.timeout
                .resolve(&serde_json::json!({ "timeout_secs": "60" })),
            None,
            "arithmetic does not get a supervising thread for asking"
        );
    }
}
