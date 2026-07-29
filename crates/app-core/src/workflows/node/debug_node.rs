use super::{PortValue, PortValues};
use crate::agent::tools::{ToolContext, ToolUiEvent};
use serde_json::Value;

/// Echo `message` to the editor Console and — unless `on_screen` is off — flash it over
/// the canvas. Carries `level` so the UI can tint both. Returns no data; exec passes through.
pub(super) fn run_print(
    config: &Value,
    inputs: &PortValues,
    sys: &ToolContext,
) -> Result<PortValues, String> {
    let message = inputs
        .get("message")
        .and_then(PortValue::as_text)
        .unwrap_or("")
        .to_string();
    let on_screen = inputs
        .get("on_screen")
        .and_then(PortValue::as_bool)
        .unwrap_or(true);
    let level = config
        .get("level")
        .and_then(Value::as_str)
        .unwrap_or("info")
        .to_string();
    let _ = sys.ui_tx.send(ToolUiEvent::Print {
        message,
        level,
        on_screen,
    });
    Ok(PortValues::new())
}
