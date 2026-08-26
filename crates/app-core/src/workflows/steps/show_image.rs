//! `ShowImage` — put a frame on the canvas, and keep it there.
//!
//! Same shape as the [`display`](super::display) sinks and the same reasoning: the UI event goes
//! down this product's own channel, because "a JPEG at a node, with a caption" is not something
//! an engine should have a word for.
//!
//! The frame is persisted as base64 alongside the label. That is deliberate and it is not free —
//! a frame is tens of kilobytes in a state row — but a debug view that goes blank on a browser
//! refresh is a debug view people stop opening, and this node exists to be looked at.

use crate::agent::tools::ToolUiEvent;
use crate::workflows::host::SentinelHost;
use crate::workflows::{compat, node};
use base64::Engine as _;
use ggraph_core::host::{Host, Slot};
use ggraph_core::spec::{NodeError, NodeStep, Step, StepCx};

pub struct ShowImage;

impl NodeStep<SentinelHost> for ShowImage {
    fn step(&self, cx: &mut StepCx<'_, SentinelHost>) -> Result<Step, NodeError> {
        let inputs = compat::from_engine_values(cx.inputs);

        // Nothing wired, or something that will not encode: show nothing rather than clearing
        // what is already on screen. A node reached with no image is usually a branch that ran
        // before its source did, not a reason to blank the last good frame.
        let Some(jpeg) = inputs
            .get("image")
            .and_then(crate::workflows::node::PortValue::as_image)
            .and_then(|b| node::to_jpeg(b).ok())
        else {
            return Ok(Step::default().arm("exec_out"));
        };

        let label = cx
            .config
            .get("label")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        cx.host.state().set(
            &cx.state_key(Slot::State),
            &serde_json::json!({
                "jpeg_b64": base64::engine::general_purpose::STANDARD.encode(&jpeg),
                "label": &label,
            }),
        );
        let _ = cx.host.tools().ui_tx.send(ToolUiEvent::ShowImage {
            graph: cx.graph,
            node: cx.node,
            label,
            jpeg,
        });

        // The Console line comes from `debug_summary`, the same place every non-cooperating node
        // gets it, rather than being restated here. A step that returns no log writes nothing at
        // all, and a node that stops describing itself in the Console is a node somebody debugs
        // by adding print statements around.
        Ok(Step::default()
            .arm("exec_out")
            .logged(crate::workflows::node::debug_summary(
                crate::workflows::domain::NodeKind::ShowImage,
                cx.config,
                &crate::workflows::compat::from_engine_values(cx.inputs),
                &Default::default(),
            )))
    }
}
