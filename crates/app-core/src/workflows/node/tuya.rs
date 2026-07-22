//! Smart-home (Tuya) nodes: control a data point and read a data point. Both resolve the
//! device from the per-tenant `tuya_devices` table (by the stable id stored in config) and
//! call the Tuya Cloud with the operator's project credentials (`TuyaConfig::from_env`).
use super::{PortValue, PortValues};
use crate::agent::tools::ToolContext;
use crate::tuya::{self, TuyaConfig};
use serde_json::Value;

fn cfg_str<'a>(config: &'a Value, key: &str) -> &'a str {
    config.get(key).and_then(Value::as_str).map(str::trim).unwrap_or("")
}

fn config_or_missing(config: &Value, sys: &ToolContext) -> Result<(TuyaConfig, tuya::TuyaDevice), String> {
    let cfg = tuya::repo::resolve_config(&sys.db, sys.tenant_id)
        .ok_or("Tuya is not connected — add your Tuya project credentials in Settings → Integrations.")?;
    let selector = cfg_str(config, "tuya_device");
    let device = tuya::repo::find_selector(&sys.db, sys.tenant_id, selector)
        .ok_or_else(|| format!("Tuya device '{selector}' not found — sync devices in Integrations."))?;
    Ok((cfg, device))
}

pub(super) fn run_tuya_control(config: &Value, inputs: &PortValues, sys: &ToolContext) -> Result<PortValues, String> {
    let (cfg, device) = config_or_missing(config, sys)?;
    let code = cfg_str(config, "tuya_code");
    if code.is_empty() {
        return Err("no Tuya data point selected — pick a 'code' (e.g. switch_1) in the node.".into());
    }
    // A wired `value` input (dynamic) wins over the config literal (the adaptive widget fallback).
    let raw = port_value_to_json(inputs.get("value"))
        .unwrap_or_else(|| config.get("value").cloned().unwrap_or_else(|| Value::String(String::new())));
    let ty = function_type(device.functions_json.as_deref(), code);
    let value = coerce_value(&raw, ty.as_deref());

    let resp = tuya::client::send_command(&cfg, &device.device_id, code, &value)?;
    let mut o = PortValues::new();
    o.insert("ok".into(), PortValue::Bool(resp.get("result").and_then(Value::as_bool).unwrap_or(true)));
    o.insert("response".into(), PortValue::Text(resp.to_string()));
    Ok(o)
}

pub(super) fn run_tuya_read_status(config: &Value, sys: &ToolContext) -> Result<PortValues, String> {
    let (cfg, device) = config_or_missing(config, sys)?;
    let code = cfg_str(config, "tuya_code");
    let status = tuya::client::get_status(&cfg, &device.device_id)?;

    let raw = serde_json::to_string(
        &status.iter().map(|(k, v)| serde_json::json!({ "code": k, "value": v })).collect::<Vec<_>>(),
    )
    .unwrap_or_default();
    let found = status.iter().find(|(k, _)| k == code).map(|(_, v)| v.clone());
    let ty = function_type(device.functions_json.as_deref(), code);
    let on = found.as_ref().map(as_bool).unwrap_or(false);
    // `value` is typed by the data point (Boolean→Bool, Integer→Num, else Text) so it wires into
    // the right port — mirroring the control node's dynamic input.
    let value = match found.as_ref() {
        Some(v) => typed_port_value(v, ty.as_deref()),
        None => PortValue::Text(String::new()),
    };

    let mut o = PortValues::new();
    o.insert("value".into(), value);
    o.insert("on".into(), PortValue::Bool(on));
    o.insert("raw".into(), PortValue::Text(raw));
    Ok(o)
}

/// The `value` output as a typed `PortValue`, matching `tuya_read_output_ports`' port type.
fn typed_port_value(v: &Value, ty: Option<&str>) -> PortValue {
    match ty {
        Some("Boolean") => PortValue::Bool(as_bool(v)),
        Some("Integer") => PortValue::Num(as_i64(v).unwrap_or(0)),
        _ => PortValue::Text(json_to_text(v)),
    }
}

/// The Tuya type name (`Boolean`/`Integer`/`Enum`/`String`) of `code`, from `functions_json`.
fn function_type(functions_json: Option<&str>, code: &str) -> Option<String> {
    let arr: Vec<Value> = serde_json::from_str(functions_json?).ok()?;
    arr.into_iter().find_map(|f| {
        (f.get("code").and_then(Value::as_str) == Some(code))
            .then(|| f.get("type").and_then(Value::as_str).map(str::to_string))
            .flatten()
    })
}

/// Coerce the config `value` literal to the JSON the Tuya command expects for the point's type.
/// The editor's adaptive widget usually stores the right JSON already; this is the safety net
/// (and the fallback when the function type is unknown — pass the literal through as-is).
fn coerce_value(raw: &Value, ty: Option<&str>) -> Value {
    match ty {
        Some("Boolean") => Value::Bool(as_bool(raw)),
        Some("Integer") => as_i64(raw).map(|n| Value::Number(n.into())).unwrap_or_else(|| Value::Number(0.into())),
        Some("Enum") | Some("String") => Value::String(json_to_text(raw)),
        _ => raw.clone(),
    }
}

fn as_bool(v: &Value) -> bool {
    match v {
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_i64().map(|i| i != 0).unwrap_or(false),
        Value::String(s) => matches!(s.trim().to_ascii_lowercase().as_str(), "true" | "on" | "1" | "yes"),
        _ => false,
    }
}

fn as_i64(v: &Value) -> Option<i64> {
    match v {
        Value::Number(n) => n.as_i64(),
        Value::String(s) => s.trim().parse().ok(),
        Value::Bool(b) => Some(*b as i64),
        _ => None,
    }
}

fn json_to_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// A wired `value` input (Bool/Num/Text) → JSON, so `coerce_value` handles it exactly like a
/// config literal. `None` when nothing is wired (caller falls back to the config literal).
fn port_value_to_json(v: Option<&PortValue>) -> Option<Value> {
    match v? {
        PortValue::Bool(b) => Some(Value::Bool(*b)),
        PortValue::Num(n) => Some(Value::Number((*n).into())),
        PortValue::Text(s) => Some(Value::String(s.clone())),
        _ => None,
    }
}
