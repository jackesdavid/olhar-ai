//! `FlipFlop` — alternate between two arms, one run to the next.
//!
//! Durable and instance-scoped like [`Cooldown`](super::cooldown), and for the same reason: a
//! flip-flop that forgot which side it was on after a restart would repeat itself, and the whole
//! point of the node is that it does not.
//!
//! The state holds which branch fires NEXT, not which one just fired. Missing state means a
//! first run, and a first run fires `a` — so `is_a` defaults to true rather than to false, which
//! would make the very first pass take the second branch.

use crate::workflows::host::SentinelHost;
use ggraph_core::host::{Host, Slot};
use ggraph_core::spec::{NodeError, NodeStep, Step, StepCx};
use ggraph_core::{PortName, Value};

pub struct FlipFlop;

impl NodeStep<SentinelHost> for FlipFlop {
    fn step(&self, cx: &mut StepCx<'_, SentinelHost>) -> Result<Step, NodeError> {
        let key = cx.state_key(Slot::State);
        let is_a = cx
            .host
            .state()
            .get(&key)
            .and_then(|v| v.get("is_a").and_then(serde_json::Value::as_bool))
            .unwrap_or(true);

        cx.host
            .state()
            .set(&key, &serde_json::json!({ "is_a": !is_a }));

        let mut out = ggraph_core::PortValues::new();
        out.insert(PortName::new("is_a"), Value::Bool(is_a));

        let arm = if is_a { "a" } else { "b" };
        Ok(Step::outputs(out)
            .arm(arm)
            .logged(format!("Flip Flop → {arm}")))
    }
}
