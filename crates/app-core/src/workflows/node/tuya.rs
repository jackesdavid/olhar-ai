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
    let cfg = TuyaConfig::from_env()
        .ok_or("Tuya is not configured — set TUYA_ACCESS_ID / TUYA_ACCESS_KEY on the server.")?;
    let selector = cfg_str(config, "tuya_device");
    let device = tuya::repo::find_selector(&sys.db, sys.tenant_id, selector)
        .ok_or_else(|| format!("Tuya device '{selector}' not found — sync devices in Integrations."))?;
    Ok((cfg, device))
}

pub(super) fn run_tuya_control(config: &Value, sys: &ToolContext) -> Result<PortValues, String> {
    let (cfg, device) = config_or_missing(config, sys)?;
    let code = cfg_str(config, "tuya_code");
    if code.is_empty() {
        return Err("no Tuya data point selected — pick a 'code' (e.g. switch_1) in the node.".into());
    }
    let raw = config.get("value").cloned().unwrap_or_else(|| Value::String(String::new()));
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
    let (value_text, on) = match &found {
        Some(v) => (json_to_text(v), as_bool(v)),
        None => (String::new(), false),
    };

    let mut o = PortValues::new();
    o.insert("value".into(), PortValue::Text(value_text));
    o.insert("on".into(), PortValue::Bool(on));
    o.insert("raw".into(), PortValue::Text(raw));
    Ok(o)
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
