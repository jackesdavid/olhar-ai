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
    // Optional guiding question (wired input or config literal). When present, the answer addresses
    // it about the frame; otherwise we produce the generic scene caption.
    let question = inputs
        .get("question")
        .and_then(PortValue::as_text)
        .or_else(|| config.get("question").and_then(Value::as_str))
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let description = match question {
        Some(q) => {
            let prompt = format!(
                "This is a still frame from a security camera named \"{label}\". Answer this about \
                 what's visible, concisely and factually (no preamble): {q}"
            );
            sys.vision.ask_text(&prompt, Some(bytes))?
        }
        None => sys.vision.describe(bytes, label)?,
    };
    let mut o = PortValues::new();
    o.insert("description".into(), PortValue::Text(description));
    o.insert("ok".into(), PortValue::Bool(true));
    Ok(o)
}

/// AI Answer: single-turn Claude — a free-text prompt (+ optional image) → a text `response`.
/// No tool-use loop (that's the chat agent's job); one round-trip keeps it fast enough for a
/// Telegram reply. Mirrors `run_describe_image` but with a caller-supplied prompt.
pub(super) fn run_ai_answer(config: &Value, inputs: &PortValues, sys: &ToolContext) -> Result<PortValues, String> {
    let prompt = inputs
        .get("prompt")
        .and_then(PortValue::as_text)
        .or_else(|| config.get("prompt").and_then(Value::as_str))
        .filter(|s| !s.trim().is_empty())
        .ok_or("AI Answer: prompt is required (wire it or set it in config)")?;
    let image = inputs.get("image").and_then(PortValue::as_image);
    let response = sys.vision.ask_text(prompt, image)?;
    let mut o = PortValues::new();
    o.insert("response".into(), PortValue::Text(response));
    o.insert("ok".into(), PortValue::Bool(true));
    Ok(o)
}

/// AI Switch: semantic router. Classifies the `text` input into one of the node's user-defined
/// `arms` (or none) via one LLM call, and stores the chosen arm label in `o["arm"]`. The engine
/// reads that in `forward_epoch` and fires the matching exec arm — the `String switch` machinery,
/// but the arm is picked by meaning instead of exact match. On no match / any error → `"default"`
/// (the unknown branch), so the graph never dead-ends.
pub(super) fn run_ai_switch(config: &Value, inputs: &PortValues, sys: &ToolContext) -> Result<PortValues, String> {
    let text = inputs
        .get("text")
        .and_then(PortValue::as_text)
        .or_else(|| config.get("text").and_then(Value::as_str))
        .unwrap_or("")
        .trim()
        .to_string();
    let arms = crate::workflows::domain::string_switch_arms(config);
    let arm = if text.is_empty() || arms.is_empty() {
        "default".to_string()
    } else {
        match sys.vision.classify(&text, &arms) {
            Ok(Some(label)) => label,
            Ok(None) => "default".to_string(),
            Err(e) => {
                log::warn!("AI Switch classify failed ({e}); routing to default");
                "default".to_string()
            }
        }
    };
    let mut o = PortValues::new();
    o.insert("arm".into(), PortValue::Text(arm));
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
