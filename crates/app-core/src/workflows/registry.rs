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
    if k == NodeKind::ForEach {
        return declared.stepping(super::steps::for_each::ForEach);
    }
    if k == NodeKind::Cooldown {
        return declared.stepping(super::steps::cooldown::Cooldown);
    }
    if k == NodeKind::FlipFlop {
        return declared.stepping(super::steps::flip_flop::FlipFlop);
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
    // `node_timeout` reads the per-node override out of config; the registry declares the
    // default, so it is asked with an empty one.
    match node::node_timeout(k, &Json::Object(Default::default())) {
        Some(d) => Timeout::Secs(d.as_secs().max(1)),
        None => Timeout::Inline,
    }
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

    /// Every kind has something to run. `Behavior::Inert` would be a lie here: these nodes all
    /// do something, and a spec that says otherwise is one the scheduler swap would silently
    /// turn into a no-op.
    #[test]
    fn every_kind_has_an_executable_behaviour() {
        let inert: Vec<&str> = registry()
            .all()
            .filter(|s| matches!(s.behavior, Behavior::Inert))
            .map(|s| s.id.as_str())
            .collect();
        assert!(inert.is_empty(), "no behaviour registered for {inert:?}");
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
