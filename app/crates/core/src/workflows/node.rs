//! Workflows node layer — the isolation boundary. Ported from the ratatui
//! `src/workflows/node.rs` and re-pointed at the **web** `ToolContext` (devices +
//! app-data paths + resolved Telegram config + the UI-effect channel).
//!
//! A node declares typed ports; the engine resolves its inputs and hands it ONLY
//! its own `PortValues`, taking back ONLY its declared outputs. A node never sees
//! the graph or another node.
//!
//! `PortValue` is a typed enum (not `serde_json::Value`) so an `Image` port can
//! carry raw JPEG bytes — JSON can't hold image bytes cleanly. Action nodes adapt
//! the core domain functions directly (typed in/out, no string-result parsing).
use super::domain::{NodeKind, Port, PortType};
use crate::agent::tools::{ToolContext, ToolUiEvent};
use crate::alerts::{self, AlertLevel};
use crate::device_actions as dev_actions;
use crate::map;
use crate::notification;
use serde_json::Value;
use std::collections::HashMap;

/// A file value: raw bytes plus the filename and MIME used for multipart uploads.
#[derive(Debug, Clone)]
pub struct FileVal {
    pub name: String,
    pub mime: String,
    pub bytes: Vec<u8>,
}

/// A typed value flowing along an edge / sitting on a port.
#[derive(Debug, Clone)]
pub enum PortValue {
    Text(String),
    Num(i64),
    Bool(bool),
    Image(Vec<u8>),
    /// A notification recipient (the recipient's label).
    Recipient(String),
    /// An arbitrary string→string map (HTTP headers / form fields).
    Map(Vec<(String, String)>),
    /// Any file (bytes + name + MIME).
    File(FileVal),
}

impl PortValue {
    pub fn as_text(&self) -> Option<&str> {
        match self {
            PortValue::Text(s) => Some(s),
            _ => None,
        }
    }
    pub fn as_num(&self) -> Option<i64> {
        match self {
            PortValue::Num(n) => Some(*n),
            _ => None,
        }
    }
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            PortValue::Bool(b) => Some(*b),
            _ => None,
        }
    }
    pub fn as_image(&self) -> Option<&[u8]> {
        match self {
            PortValue::Image(b) => Some(b),
            _ => None,
        }
    }
    pub fn as_recipient(&self) -> Option<&str> {
        match self {
            PortValue::Recipient(s) => Some(s),
            _ => None,
        }
    }
    pub fn as_map(&self) -> Option<&[(String, String)]> {
        match self {
            PortValue::Map(m) => Some(m),
            _ => None,
        }
    }
    pub fn as_file(&self) -> Option<&FileVal> {
        match self {
            PortValue::File(f) => Some(f),
            _ => None,
        }
    }
}

/// Read a file path into a `FileVal` (bytes + basename + guessed MIME).
fn read_file_val(path: &str) -> Option<FileVal> {
    let bytes = std::fs::read(path).ok()?;
    let name = std::path::Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("file")
        .to_string();
    let ext = std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let mime = match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "pdf" => "application/pdf",
        "json" => "application/json",
        "txt" | "log" => "text/plain",
        "csv" => "text/csv",
        "zip" => "application/zip",
        "mp4" => "video/mp4",
        "mp3" => "audio/mpeg",
        _ => "application/octet-stream",
    }
    .to_string();
    Some(FileVal { name, mime, bytes })
}

/// One node's resolved port values (inputs the engine assembles, or outputs a node returns).
pub type PortValues = HashMap<String, PortValue>;

/// Convert a node's config literal into a typed value for an UNCONNECTED input port.
/// Empty text / missing keys / `Image` ports yield `None` (treated as "not provided").
pub fn literal_for(config: &Value, port: Port) -> Option<PortValue> {
    let v = config.get(port.name)?;
    match port.ty {
        PortType::Text => {
            let s = v.as_str()?;
            if s.trim().is_empty() {
                None
            } else {
                Some(PortValue::Text(s.to_string()))
            }
        }
        PortType::Num => {
            if let Some(n) = v.as_i64() {
                Some(PortValue::Num(n))
            } else {
                v.as_str().and_then(|s| s.trim().parse::<i64>().ok()).map(PortValue::Num)
            }
        }
        PortType::Bool => v.as_bool().map(PortValue::Bool),
        // An image literal is a file path the user picked; read it at run time.
        PortType::Image => v
            .as_str()
            .filter(|s| !s.trim().is_empty())
            .and_then(|p| std::fs::read(p).ok())
            .map(PortValue::Image),
        // A recipient literal is the recipient's label (also produced by GetRecipient).
        PortType::Recipient => {
            let s = v.as_str()?;
            if s.trim().is_empty() {
                None
            } else {
                Some(PortValue::Recipient(s.to_string()))
            }
        }
        // A map literal is a JSON object of string→string in config.
        PortType::Map => {
            let obj = v.as_object()?;
            if obj.is_empty() {
                return None;
            }
            let pairs = obj
                .iter()
                .map(|(k, val)| {
                    let s = val.as_str().map(str::to_string).unwrap_or_else(|| val.to_string());
                    (k.clone(), s)
                })
                .collect();
            Some(PortValue::Map(pairs))
        }
        // A file literal is a path the user picked; read it at run time.
        PortType::File => {
            v.as_str().filter(|s| !s.trim().is_empty()).and_then(read_file_val).map(PortValue::File)
        }
        // Control-flow ports never carry a value.
        PortType::Exec => None,
    }
}

/// Execute one node against its resolved inputs. Returns its declared outputs.
pub fn run_node(
    kind: NodeKind,
    config: &Value,
    inputs: &PortValues,
    sys: &ToolContext,
) -> Result<PortValues, String> {
    match kind {
        NodeKind::GetLastAlert => run_get_last_alert(sys),
        NodeKind::GetRecipient => run_get_recipient(config),
        NodeKind::CreateAlert => run_create_alert(config, inputs, sys),
        NodeKind::DeliverAlert => run_deliver_alert(inputs, sys),
        NodeKind::CameraSnapshot => run_camera_snapshot(inputs, sys),
        NodeKind::DescribeImage => run_describe_image(config, inputs),
        NodeKind::NotifyRecipient => run_notify_recipient(inputs, sys),
        NodeKind::Format => run_format(config, inputs),
        NodeKind::Wait => run_wait(config),
        NodeKind::HttpPost => run_http_post(config, inputs),
    }
}

/// POST form data to `url`. `fields` (a string map) are the form fields and
/// `headers` (a string map) are extra request headers. If a `file` is provided
/// the request is sent as `multipart/form-data` (fields as text parts + the file);
/// otherwise as `application/x-www-form-urlencoded`. Outputs status, body, ok.
fn run_http_post(_config: &Value, inputs: &PortValues) -> Result<PortValues, String> {
    let url = req_text(inputs, "url")?.to_string();
    let headers = inputs.get("headers").and_then(PortValue::as_map).unwrap_or(&[]);
    let fields = inputs.get("fields").and_then(PortValue::as_map).unwrap_or(&[]);
    let file = inputs.get("file").and_then(PortValue::as_file);

    let agent = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(30))
        .build();
    let mut req = agent.post(&url);
    for (k, v) in headers {
        req = req.set(k, v);
    }

    let result = if let Some(f) = file {
        let boundary = "----sentinelFormBoundary7MA4YWxkTrZu0gW";
        let body = multipart_body(fields, f, boundary);
        req.set("Content-Type", &format!("multipart/form-data; boundary={boundary}"))
            .send_bytes(&body)
    } else {
        let form: Vec<(&str, &str)> = fields.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        req.send_form(&form)
    };

    let (status, body, ok) = match result {
        Ok(resp) => {
            let s = resp.status();
            (s as i64, resp.into_string().unwrap_or_default(), (200..300).contains(&s))
        }
        // ureq treats 4xx/5xx as an error but still carries the response.
        Err(ureq::Error::Status(code, resp)) => {
            (code as i64, resp.into_string().unwrap_or_default(), false)
        }
        Err(e) => return Err(format!("HTTP POST to {url} failed: {e}")),
    };

    let mut o = PortValues::new();
    o.insert("status".into(), PortValue::Num(status));
    o.insert("response".into(), PortValue::Text(body));
    o.insert("ok".into(), PortValue::Bool(ok));
    Ok(o)
}

/// Build a `multipart/form-data` body: each field as a text part, then the file.
fn multipart_body(fields: &[(String, String)], file: &FileVal, boundary: &str) -> Vec<u8> {
    let mut body = Vec::with_capacity(file.bytes.len() + 1024);
    for (k, v) in fields {
        body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        body.extend_from_slice(
            format!("Content-Disposition: form-data; name=\"{k}\"\r\n\r\n").as_bytes(),
        );
        body.extend_from_slice(v.as_bytes());
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    body.extend_from_slice(
        format!(
            "Content-Disposition: form-data; name=\"file\"; filename=\"{}\"\r\n",
            file.name
        )
        .as_bytes(),
    );
    body.extend_from_slice(format!("Content-Type: {}\r\n\r\n", file.mime).as_bytes());
    body.extend_from_slice(&file.bytes);
    body.extend_from_slice(b"\r\n");
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    body
}

/// Delay the run by `seconds` (clamped 0..=3600). A pure control-flow node: it
/// has no data ports — its exec_out simply fires the next node after the delay.
/// The node stays active for the whole delay since the engine runs it synchronously.
fn run_wait(config: &Value) -> Result<PortValues, String> {
    let secs = config
        .get("seconds")
        .and_then(|v| v.as_i64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok())))
        .unwrap_or(1)
        .clamp(0, 3600);
    std::thread::sleep(std::time::Duration::from_secs(secs as u64));
    Ok(PortValues::new())
}

/// Output the recipient chosen in this node's config as a typed `Recipient`.
fn run_get_recipient(config: &Value) -> Result<PortValues, String> {
    let label = config.get("recipient").and_then(Value::as_str).unwrap_or("").trim().to_string();
    let ok = !label.is_empty();
    let mut o = PortValues::new();
    o.insert("recipient".into(), PortValue::Recipient(label));
    o.insert("ok".into(), PortValue::Bool(ok));
    Ok(o)
}

fn req_text<'a>(inputs: &'a PortValues, name: &str) -> Result<&'a str, String> {
    inputs
        .get(name)
        .and_then(PortValue::as_text)
        .ok_or_else(|| format!("missing required input '{name}'"))
}

fn run_get_last_alert(sys: &ToolContext) -> Result<PortValues, String> {
    let mut o = PortValues::new();
    let alerts = sys.alerts.lock().unwrap();
    match alerts.last() {
        Some(a) => {
            o.insert("id".into(), PortValue::Num(a.id as i64));
            o.insert("level".into(), PortValue::Text(a.level.label().to_string()));
            o.insert("message".into(), PortValue::Text(a.message.clone()));
            o.insert("room".into(), PortValue::Text(a.room_name.clone().unwrap_or_default()));
            o.insert("source".into(), PortValue::Text(a.source.clone()));
            o.insert("created_at".into(), PortValue::Text(a.created_at.clone()));
            o.insert("ok".into(), PortValue::Bool(true));
        }
        None => {
            o.insert("ok".into(), PortValue::Bool(false));
        }
    }
    Ok(o)
}

fn run_create_alert(
    config: &Value,
    inputs: &PortValues,
    sys: &ToolContext,
) -> Result<PortValues, String> {
    let message = req_text(inputs, "message")?.to_string();
    let level = config
        .get("level")
        .and_then(Value::as_str)
        .and_then(AlertLevel::parse)
        .unwrap_or_default();
    let source = config
        .get("source")
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .unwrap_or("workflow")
        .to_string();
    let (room_id, room_name) = match inputs
        .get("room")
        .and_then(PortValue::as_text)
        .filter(|s| !s.trim().is_empty())
    {
        Some(name) => {
            let rooms = map::load_map(&sys.map_path);
            match rooms.iter().find(|r| r.name.eq_ignore_ascii_case(name.trim())) {
                Some(r) => (Some(r.id), Some(r.name.clone())),
                None => (None, Some(name.to_string())),
            }
        }
        None => (None, None),
    };
    let a = alerts::create(&sys.alerts, &sys.alerts_path, level, message, room_id, room_name, source);
    let _ = sys.ui_tx.send(ToolUiEvent::AlertsChanged);
    let mut o = PortValues::new();
    o.insert("id".into(), PortValue::Num(a.id as i64));
    o.insert(
        "result".into(),
        PortValue::Text(format!("Raised alert {} [{}]", a.id, a.level.label())),
    );
    o.insert("ok".into(), PortValue::Bool(true));
    Ok(o)
}

fn run_deliver_alert(inputs: &PortValues, sys: &ToolContext) -> Result<PortValues, String> {
    let id = inputs
        .get("id")
        .and_then(PortValue::as_num)
        .ok_or("missing required input 'id'")? as u64;
    let cfg = sys
        .notif_cfg
        .as_ref()
        .ok_or("Notifications are not configured (set the Telegram bot token in Beacons).")?;
    let result = alerts::deliver(&sys.alerts, &sys.alerts_path, &sys.notif, cfg, id)?;
    let _ = sys.ui_tx.send(ToolUiEvent::AlertsChanged);
    let mut o = PortValues::new();
    o.insert("result".into(), PortValue::Text(result));
    o.insert("ok".into(), PortValue::Bool(true));
    Ok(o)
}

fn run_camera_snapshot(inputs: &PortValues, sys: &ToolContext) -> Result<PortValues, String> {
    let camera = req_text(inputs, "camera")?;
    let device = {
        let devices = sys.devices.lock().unwrap();
        let idx = dev_actions::resolve_device(&devices, camera).map_err(|e| e.to_string())?;
        devices[idx].clone()
    };
    let bytes = dev_actions::capture_snapshot_bytes(&device)?;
    // Mirror the chat tool: also surface the snapshot in the UI.
    if let Ok(jpeg) = to_jpeg(&bytes) {
        let _ = sys.ui_tx.send(ToolUiEvent::Snapshot { camera: device.name.clone(), jpeg });
    }
    let mut o = PortValues::new();
    o.insert("image".into(), PortValue::Image(bytes));
    o.insert("result".into(), PortValue::Text(format!("Snapshot of '{}' captured", device.name)));
    o.insert("ok".into(), PortValue::Bool(true));
    Ok(o)
}

fn run_describe_image(config: &Value, inputs: &PortValues) -> Result<PortValues, String> {
    let bytes = inputs
        .get("image")
        .and_then(PortValue::as_image)
        .ok_or("missing required input 'image'")?;
    let label = config
        .get("label")
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .unwrap_or("camera");
    let description = crate::agent::vision::describe_image(bytes, label)?;
    let mut o = PortValues::new();
    o.insert("description".into(), PortValue::Text(description));
    o.insert("ok".into(), PortValue::Bool(true));
    Ok(o)
}

fn run_notify_recipient(inputs: &PortValues, sys: &ToolContext) -> Result<PortValues, String> {
    let message = req_text(inputs, "message")?;
    // A wired Recipient picks the target; unwired → auto (the sole enabled one).
    let target = inputs
        .get("recipient")
        .and_then(PortValue::as_recipient)
        .filter(|s| !s.trim().is_empty());
    let image = inputs.get("image").and_then(PortValue::as_image).map(|b| b.to_vec());
    let cfg = sys
        .notif_cfg
        .as_ref()
        .ok_or("Notifications are not configured (set the Telegram bot token in Beacons).")?;
    let result = notification::dispatch(&sys.notif, cfg, target, message, image)?;
    let mut o = PortValues::new();
    o.insert("result".into(), PortValue::Text(result));
    o.insert("ok".into(), PortValue::Bool(true));
    Ok(o)
}

fn run_format(config: &Value, inputs: &PortValues) -> Result<PortValues, String> {
    let template = config.get("template").and_then(Value::as_str).unwrap_or("");
    let get = |k: &str| inputs.get(k).and_then(PortValue::as_text).unwrap_or("").to_string();
    let text = template
        .replace("{p1}", &get("p1"))
        .replace("{p2}", &get("p2"))
        .replace("{p3}", &get("p3"));
    let mut o = PortValues::new();
    o.insert("text".into(), PortValue::Text(text));
    Ok(o)
}

/// Re-encode snapshot bytes to JPEG for the UI event (same helper as the chat tool).
fn to_jpeg(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let img = image::load_from_memory(bytes).map_err(|e| format!("decode snapshot: {e}"))?;
    let mut out = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Jpeg)
        .map_err(|e| format!("encode snapshot: {e}"))?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::super::domain::NodeKind;
    use super::*;
    use serde_json::json;

    fn test_ctx() -> ToolContext {
        let (tx, _rx) = std::sync::mpsc::channel();
        ToolContext {
            devices: std::sync::Arc::new(std::sync::Mutex::new(vec![])),
            devices_path: std::env::temp_dir().join("wf_test_devices.json"),
            alerts: std::sync::Arc::new(std::sync::Mutex::new(vec![])),
            alerts_path: std::env::temp_dir().join("wf_test_alerts.json"),
            notif: std::sync::Arc::new(std::sync::Mutex::new(vec![])),
            map_path: std::env::temp_dir().join("wf_test_map.json"),
            notif_cfg: None,
            ui_tx: tx,
        }
    }

    #[test]
    fn literal_for_coerces_and_skips_empty() {
        let cfg = json!({ "camera": "front", "blank": "  ", "n": "7" });
        let text = Port { name: "camera", ty: PortType::Text, required: true };
        let blank = Port { name: "blank", ty: PortType::Text, required: false };
        let num = Port { name: "n", ty: PortType::Num, required: true };
        assert_eq!(literal_for(&cfg, text).unwrap().as_text(), Some("front"));
        assert!(literal_for(&cfg, blank).is_none());
        assert_eq!(literal_for(&cfg, num).unwrap().as_num(), Some(7));
    }

    #[test]
    fn literal_for_parses_a_string_map() {
        let cfg = json!({ "headers": { "Authorization": "Bearer x", "X-Env": "prod" }, "empty": {} });
        let map = Port { name: "headers", ty: PortType::Map, required: false };
        let empty = Port { name: "empty", ty: PortType::Map, required: false };
        let got = literal_for(&cfg, map).unwrap();
        let pairs = got.as_map().unwrap();
        assert_eq!(pairs.len(), 2);
        assert!(pairs.contains(&("Authorization".into(), "Bearer x".into())));
        assert!(literal_for(&cfg, empty).is_none()); // empty map → not provided
    }

    #[test]
    fn http_post_requires_a_url_before_any_request() {
        let err = run_node(NodeKind::HttpPost, &json!({ "url": "  " }), &PortValues::new(), &test_ctx())
            .unwrap_err();
        assert!(err.contains("missing required input 'url'"), "got: {err}");
    }

    #[test]
    fn wait_is_pure_control_flow_no_data_outputs() {
        // seconds=0 → no real delay; a control-flow node emits no data ports.
        let out = run_node(NodeKind::Wait, &json!({ "seconds": "0" }), &PortValues::new(), &test_ctx())
            .unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn format_substitutes_present_inputs_only() {
        let cfg = json!({ "template": "{p1} — {p2}" });
        let mut inputs = PortValues::new();
        inputs.insert("p1".into(), PortValue::Text("door".into()));
        let out = run_node(NodeKind::Format, &cfg, &inputs, &test_ctx()).unwrap();
        assert_eq!(out["text"].as_text(), Some("door — "));
    }

    #[test]
    fn create_alert_requires_message_before_any_io() {
        let ctx = test_ctx();
        let cfg = NodeKind::CreateAlert.default_config();
        let err = run_node(NodeKind::CreateAlert, &cfg, &PortValues::new(), &ctx).unwrap_err();
        assert!(err.contains("missing required input 'message'"), "got: {err}");
        assert!(ctx.alerts.lock().unwrap().is_empty());
    }
}
