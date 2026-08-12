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
    let name = config
        .get("variable")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
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
        // Table is a reference resolved from sys at run start — no standalone zero.
        | PortType::Table
        | PortType::Embedding
        | PortType::Landmarks
        | PortType::Region
        | PortType::TrackedObject
        | PortType::TrackedObjectArray
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

/// The variable names configured on a multi node, in order. Blank and duplicate entries are
/// dropped so this agrees exactly with the ports `multi_variable_ports` derives — the ports and
/// the execution must not disagree about which variables the node has.
pub(super) fn multi_var_names(config: &Value) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    let Some(list) = config.get("variables").and_then(|v| v.as_array()) else {
        return names;
    };
    for entry in list {
        if let Some(n) = entry.get("name").and_then(|v| v.as_str()) {
            let n = n.trim();
            if !n.is_empty() && !names.iter().any(|k| k == n) {
                names.push(n.to_string());
            }
        }
    }
    names
}

/// The declared type of one entry, for the zero-value fallback on the Get side.
fn multi_var_type(config: &Value, name: &str) -> PortType {
    config
        .get("variables")
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
        .find(|e| e.get("name").and_then(|v| v.as_str()).map(str::trim) == Some(name))
        .and_then(|e| e.get("type").cloned())
        .and_then(|t| serde_json::from_value::<PortType>(t).ok())
        .unwrap_or(PortType::Text)
}

/// Write every WIRED input into its variable. Exec passes through.
///
/// A configured variable whose input is missing is SKIPPED, not zeroed. "Not wired" means "leave
/// this one alone", which is what lets the same node sit on two branches that each write a
/// different subset — zeroing would clobber whatever an earlier branch had set.
pub(super) fn run_multi_set_variables(
    config: &Value,
    inputs: &PortValues,
    sys: &ToolContext,
) -> Result<PortValues, String> {
    let names = multi_var_names(config);
    if names.is_empty() {
        return Err("Set Variables: no variables added — pick them in the properties panel".into());
    }
    let mut vars = sys.variables.lock().unwrap();
    for name in names {
        if let Some(v) = inputs.get(&name) {
            vars.insert(name, v.clone());
        }
    }
    Ok(PortValues::new())
}

/// Read every configured variable into a same-named output, falling back to the type's zero so a
/// read before any write never fails the run — identical to `run_get_variable`, N times over.
pub(super) fn run_multi_get_variables(
    config: &Value,
    sys: &ToolContext,
) -> Result<PortValues, String> {
    let names = multi_var_names(config);
    if names.is_empty() {
        return Err("Get Variables: no variables added — pick them in the properties panel".into());
    }
    let stored = sys.variables.lock().unwrap();
    let mut o = PortValues::new();
    for name in names {
        let v = stored
            .get(&name)
            .cloned()
            .or_else(|| zero_value(multi_var_type(config, &name)));
        if let Some(v) = v {
            o.insert(name, v);
        }
    }
    Ok(o)
}
