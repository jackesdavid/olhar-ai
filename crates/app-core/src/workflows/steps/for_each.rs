//! `ForEach` — run the body once per item, then carry on.
//!
//! The scheduler drives this inline today: it keeps the index in a `loop_state` map, fires
//! `loop_body`, and pushes the node onto the next epoch's re-entry list. All three of those are
//! what [`Step`] expresses, and the index maps onto the engine's per-node `scratch` — run-scoped
//! and private, which is right: a loop that resumed mid-iteration after a restart would repeat
//! whatever the body already did.

use crate::workflows::compat;
use crate::workflows::host::SentinelHost;
use crate::workflows::node::PortValue;
use ggraph_core::spec::{NodeError, NodeStep, Step, StepCx};
use ggraph_core::{PortName, Value};

pub struct ForEach;

impl NodeStep<SentinelHost> for ForEach {
    fn step(&self, cx: &mut StepCx<'_, SentinelHost>) -> Result<Step, NodeError> {
        let inputs = compat::from_engine_values(cx.inputs);

        // Items come from a wired array, a wired text list, a single wired value (one pass), or
        // the config text when nothing is wired. Unchanged from the inline version — this is a
        // move, and a move that quietly altered what counts as a list would be a bad one.
        let items: Vec<PortValue> = match inputs.get("items") {
            Some(PortValue::List(v)) => v.clone(),
            Some(PortValue::Text(t)) => crate::workflows::node::parse_text_items(t),
            Some(other) => vec![other.clone()],
            None => crate::workflows::node::parse_text_items(
                cx.config
                    .get("items")
                    .and_then(|v| v.as_str())
                    .unwrap_or(""),
            ),
        };

        let i = cx
            .scratch
            .get("i")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0) as usize;

        let mut out = ggraph_core::PortValues::new();
        out.insert(PortName::new("index"), Value::int(i as i64));

        if i >= items.len() {
            // Reset, so a loop reached again inside an outer loop starts over rather than
            // reporting itself already finished.
            *cx.scratch = serde_json::json!({});
            return Ok(Step::outputs(out)
                .arm("completed")
                .logged(format!("{} item(s)", items.len())));
        }

        cx.scratch["i"] = serde_json::json!(i + 1);
        out.insert(PortName::new("item"), compat::to_engine(&items[i]));
        Ok(Step::outputs(out).arm("loop_body").reentering())
    }
}
