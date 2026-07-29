use super::{PortValue, PortValues};
use serde_json::Value;

pub(super) fn run_format(config: &Value, inputs: &PortValues) -> Result<PortValues, String> {
    let template = config.get("template").and_then(Value::as_str).unwrap_or("");
    let get = |k: &str| {
        inputs
            .get(k)
            .and_then(PortValue::as_text)
            .unwrap_or("")
            .to_string()
    };
    let text = template
        .replace("{p1}", &get("p1"))
        .replace("{p2}", &get("p2"))
        .replace("{p3}", &get("p3"));
    let mut o = PortValues::new();
    o.insert("text".into(), PortValue::Text(text));
    Ok(o)
}
