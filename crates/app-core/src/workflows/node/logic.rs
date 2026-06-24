use super::{PortValue, PortValues};
use serde_json::Value;

pub(super) fn run_compare(config: &Value, inputs: &PortValues) -> Result<PortValues, String> {
    let a = inputs.get("a").and_then(PortValue::as_num).unwrap_or(0);
    let b = inputs.get("b").and_then(PortValue::as_num).unwrap_or(0);
    let op = config.get("operator").and_then(Value::as_str).unwrap_or("==");
    let result = match op {
        ">"  => a > b,
        ">=" => a >= b,
        "<"  => a < b,
        "<=" => a <= b,
        "==" => a == b,
        "!=" => a != b,
        _ => return Err(format!("Compare: unknown operator '{op}' (use >, >=, <, <=, ==, !=)")),
    };
    let mut o = PortValues::new();
    o.insert("result".into(), PortValue::Bool(result));
    Ok(o)
}

/// Numeric comparison with a fixed operator — shared by the six Less/Greater/Equal/Not-Equal nodes.
pub(super) fn run_num_cmp(config: &Value, inputs: &PortValues, op: impl FnOnce(i64, i64) -> bool) -> Result<PortValues, String> {
    let a = inputs.get("a").and_then(PortValue::as_num)
        .or_else(|| config.get("a").and_then(|v| v.as_i64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))))
        .unwrap_or(0);
    let b = inputs.get("b").and_then(PortValue::as_num)
        .or_else(|| config.get("b").and_then(|v| v.as_i64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))))
        .unwrap_or(0);
    let mut o = PortValues::new();
    o.insert("result".into(), PortValue::Bool(op(a, b)));
    Ok(o)
}

pub(super) fn run_and(config: &Value, inputs: &PortValues) -> Result<PortValues, String> {
    let bool_val = |key: &str| -> bool {
        inputs.get(key).and_then(PortValue::as_bool)
            .or_else(|| config.get(key).and_then(|v| v.as_bool().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))))
            .unwrap_or(false)
    };
    let mut o = PortValues::new();
    o.insert("result".into(), PortValue::Bool(bool_val("a") && bool_val("b")));
    Ok(o)
}

pub(super) fn run_or(config: &Value, inputs: &PortValues) -> Result<PortValues, String> {
    let bool_val = |key: &str| -> bool {
        inputs.get(key).and_then(PortValue::as_bool)
            .or_else(|| config.get(key).and_then(|v| v.as_bool().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))))
            .unwrap_or(false)
    };
    let mut o = PortValues::new();
    o.insert("result".into(), PortValue::Bool(bool_val("a") || bool_val("b")));
    Ok(o)
}

pub(super) fn run_not(config: &Value, inputs: &PortValues) -> Result<PortValues, String> {
    let a = inputs.get("a").and_then(PortValue::as_bool)
        .or_else(|| config.get("a").and_then(|v| v.as_bool().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))))
        .unwrap_or(false);
    let mut o = PortValues::new();
    o.insert("result".into(), PortValue::Bool(!a));
    Ok(o)
}

pub(super) fn run_text_compare(config: &Value, inputs: &PortValues) -> Result<PortValues, String> {
    let text_val = |key: &str| -> String {
        inputs.get(key).and_then(PortValue::as_text)
            .or_else(|| config.get(key).and_then(|v| v.as_str()))
            .unwrap_or("")
            .to_string()
    };
    let a = text_val("a");
    let b = text_val("b");
    let op = config.get("operator").and_then(Value::as_str).unwrap_or("eq");
    let result = match op {
        "eq"          => a == b,
        "ne"          => a != b,
        "contains"    => a.contains(b.as_str()),
        "starts_with" => a.starts_with(b.as_str()),
        "ends_with"   => a.ends_with(b.as_str()),
        _             => return Err(format!("unknown text_compare operator: '{op}'")),
    };
    let mut o = PortValues::new();
    o.insert("result".into(), PortValue::Bool(result));
    Ok(o)
}

pub(super) fn run_num_to_text(inputs: &PortValues) -> Result<PortValues, String> {
    let v = inputs.get("value").and_then(PortValue::as_num).unwrap_or(0);
    let mut o = PortValues::new();
    o.insert("text".into(), PortValue::Text(v.to_string()));
    Ok(o)
}

pub(super) fn run_bool_to_text(inputs: &PortValues) -> Result<PortValues, String> {
    let v = inputs.get("value").and_then(PortValue::as_bool).unwrap_or(false);
    let mut o = PortValues::new();
    o.insert("text".into(), PortValue::Text(if v { "true" } else { "false" }.into()));
    Ok(o)
}
