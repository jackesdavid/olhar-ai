use super::{PortValue, PortValues};
use crate::agent::tools::ToolContext;
use serde_json::Value;

pub(super) fn run_describe_image(config: &Value, inputs: &PortValues, sys: &ToolContext) -> Result<PortValues, String> {
    let bytes = inputs
        .get("image")
        .and_then(PortValue::as_image)
        .ok_or("missing required input 'image'")?;
    let label = config
        .get("label")
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .unwrap_or("camera");
    let description = sys.vision.describe(bytes, label)?;
    let mut o = PortValues::new();
    o.insert("description".into(), PortValue::Text(description));
    o.insert("ok".into(), PortValue::Bool(true));
    Ok(o)
}

/// AI Branch: asks the vision model a yes/no question about an optional image and returns
/// `result: Bool`. The engine routes exec to `true` or `false` based on this result,
/// identically to how it handles the `If` node but reading from outputs instead of inputs.
pub(super) fn run_ask_ai(config: &Value, inputs: &PortValues, sys: &ToolContext) -> Result<PortValues, String> {
    let question = inputs
        .get("question")
        .and_then(PortValue::as_text)
        .or_else(|| config.get("question").and_then(|v| v.as_str()))
        .filter(|s| !s.trim().is_empty())
        .ok_or("AI Branch: question is required (wire it or set it in config)")?;
    let image = inputs.get("image").and_then(PortValue::as_image);
    let answer = sys.vision.ask_yes_no(question, image)?;
    let mut o = PortValues::new();
    o.insert("result".into(), PortValue::Bool(answer));
    Ok(o)
}
