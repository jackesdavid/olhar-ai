//! `Text` and `Gauge` — show a value on the canvas, and remember it.
//!
//! Two kinds, one behaviour: read `value`, persist it, push it to the editor. They are separate
//! kinds because they *render* differently, and rendering is the editor's business.
//!
//! Note what this reaches and how. The UI event is `ToolUiEvent::ShowValue`, sent straight down
//! this product's own channel through [`SentinelHost::tools`] — not through an engine trait. The
//! engine's `UiEvent` has two variants and would have to grow one per product to carry this,
//! which is exactly the coupling the extraction exists to avoid. A product's own node reaching
//! its own world directly is the design working, not a shortcut around it.
//!
//! The value is persisted so it survives a refresh and a flow restart: a gauge that goes blank
//! when a pod recycles is one nobody trusts.

use crate::agent::tools::ToolUiEvent;
use crate::workflows::compat;
use crate::workflows::host::SentinelHost;
use crate::workflows::node::PortValue;
use ggraph_core::host::{Host, Slot};
use ggraph_core::spec::{NodeError, NodeStep, Step, StepCx};

/// Carries its kind, because one behaviour serves two of them and `StepCx` does not say which.
/// The spec knows at registration time, so the behaviour is built with it — the same shape the
/// adapter uses.
pub struct Display(pub crate::workflows::domain::NodeKind);

impl NodeStep<SentinelHost> for Display {
    fn step(&self, cx: &mut StepCx<'_, SentinelHost>) -> Result<Step, NodeError> {
        let inputs = compat::from_engine_values(cx.inputs);

        // Only the scalars render. Anything else — an image, a list, a camera — has no reading
        // on a gauge, and writing `null` would blank a value that was fine a moment ago.
        let value = match inputs.get("value") {
            Some(PortValue::Text(s)) => serde_json::json!(s),
            Some(PortValue::Num(v)) => serde_json::json!(v),
            Some(PortValue::Bool(b)) => serde_json::json!(b),
            _ => serde_json::Value::Null,
        };

        if !value.is_null() {
            cx.host.state().set(
                &cx.state_key(Slot::State),
                &serde_json::json!({ "value": &value }),
            );
            let _ = cx.host.tools().ui_tx.send(ToolUiEvent::ShowValue {
                graph: cx.graph,
                node: cx.node,
                kind: self.0.slug().to_string(),
                value,
            });
        }

        // The Console line comes from `debug_summary`, the same place every non-cooperating node
        // gets it, rather than being restated here. A step that returns no log writes nothing at
        // all, and a node that stops describing itself in the Console is a node somebody debugs
        // by adding print statements around.
        Ok(Step::default()
            .arm("exec_out")
            .logged(crate::workflows::node::debug_summary(
                self.0,
                cx.config,
                &crate::workflows::compat::from_engine_values(cx.inputs),
                &Default::default(),
            )))
    }
}
