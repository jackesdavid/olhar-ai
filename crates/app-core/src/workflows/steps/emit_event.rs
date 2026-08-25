//! `EmitEvent` — fire a named event that other graphs can be woken by.
//!
//! The node's gathered inputs ARE the payload, one per declared field. That is why its input
//! ports are config-derived: the event's schema is the node's shape.

use crate::workflows::compat;
use crate::workflows::host::SentinelHost;
use ggraph_core::host::Host;
use ggraph_core::spec::{NodeError, NodeStep, Step, StepCx};

pub struct EmitEvent;

impl NodeStep<SentinelHost> for EmitEvent {
    fn step(&self, cx: &mut StepCx<'_, SentinelHost>) -> Result<Step, NodeError> {
        let Some(event) = cx
            .config
            .get("event")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
        else {
            // No event chosen. Not an error: a node dropped on the canvas and not yet configured
            // is a normal state for a graph somebody is still building.
            return Ok(Step::default().arm("exec_out"));
        };

        let tools = cx.host.tools();
        let Some(emitter) = tools.emitter.as_ref() else {
            // Nothing is listening because nothing CAN listen — this context has no event bus.
            // Saying so beats reporting a fire that reached nobody, which is the same outcome as
            // a broken listener and impossible to tell apart afterwards.
            return Err(NodeError::new(
                "this run has no event bus, so the event was not fired",
            ));
        };

        emitter.emit(
            cx.graph,
            event,
            tools.run_id,
            &compat::from_engine_values(cx.inputs),
        );
        Ok(Step::default().arm("exec_out").logged(event.to_string()))
    }
}
