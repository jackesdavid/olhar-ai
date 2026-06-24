use super::{PortValue, PortValues, device_from_camera, jpeg_dimensions, resolve_camera_entity, to_jpeg};
use crate::agent::tools::{ToolContext, ToolUiEvent};
use serde_json::Value;

pub(super) fn run_camera_snapshot(inputs: &PortValues, sys: &ToolContext) -> Result<PortValues, String> {
    let cam = inputs.get("camera").and_then(PortValue::as_camera)
        .ok_or("missing required camera input")?;
    let device = {
        let devices = sys.devices.lock().unwrap();
        device_from_camera(cam, &devices)?.clone()
    };
    let bytes = sys.frames.snapshot(&device)?;
    if let Ok(jpeg) = to_jpeg(&bytes) {
        let _ = sys.ui_tx.send(ToolUiEvent::Snapshot { camera: device.name.clone(), jpeg });
    }
    let (w, h) = jpeg_dimensions(&bytes);
    let mut o = PortValues::new();
    o.insert("image".into(), PortValue::Image(bytes));
    o.insert("result".into(), PortValue::Text(format!("Snapshot of '{}' captured", device.name)));
    o.insert("width".into(), PortValue::Num(w));
    o.insert("height".into(), PortValue::Num(h));
    o.insert("ok".into(), PortValue::Bool(true));
    Ok(o)
}

/// Fast frame source: return the camera's most-recent warm in-memory frame (no per-call
/// camera round trip). For high-rate loops like a 1 Hz recognition graph.
pub(super) fn run_get_current_frame(config: &Value, inputs: &PortValues, sys: &ToolContext) -> Result<PortValues, String> {
    let cam = inputs.get("camera").and_then(PortValue::as_camera)
        .ok_or("missing required camera input")?;
    let device = {
        let devices = sys.devices.lock().unwrap();
        device_from_camera(cam, &devices)?.clone()
    };
    let max_age_ms = config
        .get("max_age_ms")
        .and_then(|v| v.as_u64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok())))
        .unwrap_or(2000);
    let (bytes, age_ms) = sys.frames.current(&device, max_age_ms)?;
    let (w, h) = jpeg_dimensions(&bytes);
    let mut o = PortValues::new();
    o.insert("image".into(), PortValue::Image(bytes));
    o.insert("age_ms".into(), PortValue::Num(age_ms as i64));
    o.insert("width".into(), PortValue::Num(w));
    o.insert("height".into(), PortValue::Num(h));
    o.insert("ok".into(), PortValue::Bool(true));
    Ok(o)
}

pub(super) fn run_get_camera_by_name(config: &Value, sys: &ToolContext) -> Result<PortValues, String> {
    let id = config
        .get("camera")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or("No camera selected — pick one in the node config.")?;
    let cam = resolve_camera_entity(id, sys)
        .ok_or_else(|| format!("camera '{id}' not found"))?;
    let mut o = PortValues::new();
    o.insert("camera".into(), PortValue::Camera(cam));
    Ok(o)
}

pub(super) fn run_break_camera(inputs: &PortValues) -> Result<PortValues, String> {
    let cam = inputs
        .get("camera")
        .and_then(PortValue::as_camera)
        .ok_or("missing required camera input")?;
    let mut o = PortValues::new();
    o.insert("id".into(), PortValue::Text(cam.id.clone()));
    o.insert("ip".into(), PortValue::Text(cam.ip.clone()));
    o.insert("name".into(), PortValue::Text(cam.name.clone()));
    Ok(o)
}
