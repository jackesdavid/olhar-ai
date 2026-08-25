//! `OnEvent` — the node a fired event enters the graph through.
//!
//! Two things make it unlike every other node, and both are expressible rather than special:
//!
//! **It never runs on an ordinary trigger.** A listener with nothing to listen to has not
//! happened. `StepCx::forced` is exactly the question "is this node the entry of this pass?",
//! and when the answer is no this fires no arms at all — so nothing downstream of it runs,
//! without the scheduler needing a rule about which kinds are listeners.
//!
//! **Its outputs come from the payload, not from a computation.** The delivered event carries
//! one value per declared field, and the node's output ports are derived from that same
//! declaration — which is why they are config-derived. `seed_event_payload` also upgrades the
//! typed ones: a camera arrives on the wire as an IP or an id and becomes a `CameraEntity`, a
//! bbox arrives as four numbers, an image arrives base64-encoded because JSON has no byte type.

use crate::workflows::host::SentinelHost;
use crate::workflows::{compat, domain, node};
use ggraph_core::host::Host;
use ggraph_core::spec::{NodeError, NodeStep, Step, StepCx};

pub struct OnEvent;

impl NodeStep<SentinelHost> for OnEvent {
    fn step(&self, cx: &mut StepCx<'_, SentinelHost>) -> Result<Step, NodeError> {
        if !cx.forced {
            // A listener, not an entry. No arms fire, so the branch below it stays dead — which
            // is the same outcome the scheduler's `if kind == OnEvent { forced }` rule produced,
            // said by the node instead of about it.
            return Ok(Step::default());
        }

        let payload = compat::from_engine_values(cx.entry_payload);
        let ports = domain::resolved_output_ports(domain::NodeKind::OnEvent, cx.config);
        let seeded = node::seed_event_payload(&payload, &ports, cx.host.tools());

        Ok(Step::outputs(compat::to_engine_values(&seeded)).arm("exec_out"))
    }
}
