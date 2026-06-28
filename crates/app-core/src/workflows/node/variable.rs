//! Get/Set Variable nodes — read and write a workflow's per-run variables (Unreal-style).
//! State lives on the run's `ToolContext::variables` map (one map per run), so a `Set` early in
//! the flow is visible to a `Get` anywhere downstream without wiring them together. `Get` is pure
//! (no exec); `Set` writes on activation and passes exec through.
use super::{PortValue, PortValues};
use crate::agent::tools::ToolContext;
use crate::workflows::domain::{BBox, PortType};
use serde_json::Value;

/// The variable name selected for this node (`config.variable`).
fn var_name(config: &Value) -> Result<String, String> {
    let name = config.get("variable").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
    if name.is_empty() {
        Err("no variable selected".into())
    } else {
        Ok(name)
    }
}

/// The variable's declared wire type (`config.type`), defaulting to Text.
fn var_type(config: &Value) -> PortType {
    config
        .get("type")
        .and_then(|v| serde_json::from_value::<PortType>(v.clone()).ok())
        .unwrap_or(PortType::Text)
}

/// A zero/default value for a variable read before any `Set` ran, so a `Get` never fails the run.
/// Types with no sensible zero (Image/File/Camera/Person/Face) yield `None` → the output port is
/// simply left unfilled (a downstream required input then reports the missing value clearly).
fn zero_value(ty: PortType) -> Option<PortValue> {
    Some(match ty {
        PortType::Text => PortValue::Text(String::new()),
        PortType::Num => PortValue::Num(0),
        PortType::Bool => PortValue::Bool(false),
        PortType::Recipient => PortValue::Recipient(String::new()),
        PortType::Dictionary => PortValue::Dict(Vec::new()),
        PortType::TextArray
        | PortType::NumArray
        | PortType::PersonArray
        | PortType::FaceArray => PortValue::List(Vec::new()),
        PortType::Bbox => PortValue::Bbox(BBox::from_xyxy(0, 0, 0, 0)),
        PortType::Image
        | PortType::File
        | PortType::Person
        | PortType::Face
        | PortType::Camera
        | PortType::Exec
        | PortType::Any => return None,
    })
}

/// Read the variable's current value (or its type-zero default if never set) → `value` output.
pub(super) fn run_get_variable(config: &Value, sys: &ToolContext) -> Result<PortValues, String> {
    let name = var_name(config).map_err(|e| format!("Get Variable: {e}"))?;
    let stored = sys.variables.lock().unwrap().get(&name).cloned();
    let mut o = PortValues::new();
    if let Some(v) = stored.or_else(|| zero_value(var_type(config))) {
        o.insert("value".to_string(), v);
    }
    Ok(o)
}

/// Write the `value` input into the variable. Exec passes through (no data output).
pub(super) fn run_set_variable(
    config: &Value,
    inputs: &PortValues,
    sys: &ToolContext,
) -> Result<PortValues, String> {
    let name = var_name(config).map_err(|e| format!("Set Variable: {e}"))?;
    let value = inputs
        .get("value")
        .cloned()
        .ok_or("Set Variable: no 'value' input")?;
    sys.variables.lock().unwrap().insert(name, value);
    Ok(PortValues::new())
}
