use super::{PortValue, PortValues};
use serde_json::Value;

/// Validates the `condition` Bool is present. The engine reads it to choose the true/false branch.
pub(super) fn run_if(inputs: &PortValues) -> Result<PortValues, String> {
    inputs
        .get("condition")
        .and_then(PortValue::as_bool)
        .ok_or("missing required input 'condition'")?;
    Ok(PortValues::new())
}

/// Delays the run by `seconds` (clamped 0..=3600). Pure control-flow — no data ports.
pub(super) fn run_wait(config: &Value) -> Result<PortValues, String> {
    let secs = config
        .get("seconds")
        .and_then(|v| {
            v.as_i64()
                .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
        })
        .unwrap_or(1)
        .clamp(0, 3600);
    std::thread::sleep(std::time::Duration::from_secs(secs as u64));
    Ok(PortValues::new())
}

/// Validates the required `value` Text input is present. The engine's `forward_epoch` handles
/// the actual arm routing — exactly one arm fires.
pub(super) fn run_string_switch(inputs: &PortValues) -> Result<PortValues, String> {
    inputs
        .get("value")
        .and_then(PortValue::as_text)
        .ok_or("missing required input 'value'")?;
    Ok(PortValues::new())
}

/// Validates the required `camera` Camera input is present. The engine's `forward_epoch`
/// handles arm routing — the arm whose configured camera key matches the incoming camera fires.
pub(super) fn run_camera_switch(inputs: &PortValues) -> Result<PortValues, String> {
    inputs
        .get("camera")
        .and_then(PortValue::as_camera)
        .ok_or("missing required input 'camera'")?;
    Ok(PortValues::new())
}
