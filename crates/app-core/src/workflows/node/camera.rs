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

/// A reusable named line/zone: resolves `camera` (wire override, else `config.camera`, like
/// `ObjectDetection`'s `class`) to a concrete camera ip for consistency-checking downstream, and
/// reads `name`/`points` straight from config (drawn in the property panel, not wireable —
/// there's no upstream node that would produce arbitrary geometry). Whether the result means a
/// line or a zone is purely by `points.len()`, decided by whichever `OnEvent` reads it.
pub(super) fn run_region(config: &Value, inputs: &PortValues, sys: &ToolContext) -> Result<PortValues, String> {
    let id = inputs
        .get("camera")
        .and_then(PortValue::as_camera)
        .map(|c| c.ip.clone())
        .map(Ok)
        .unwrap_or_else(|| {
            let cfg_id = config.get("camera").and_then(|v| v.as_str()).map(str::trim).filter(|s| !s.is_empty())
                .ok_or("No camera selected — pick one in the node config.")?;
            resolve_camera_entity(cfg_id, sys).map(|c| c.ip).ok_or_else(|| format!("camera '{cfg_id}' not found"))
        })?;
    let name = config.get("name").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
    if name.is_empty() {
        return Err("Region needs a name — set one in the node config.".into());
    }
    let points: Vec<(f32, f32)> = config
        .get("points")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|p| {
                    let a = p.as_array()?;
                    Some((a.first()?.as_f64()? as f32, a.get(1)?.as_f64()? as f32))
                })
                .collect()
        })
        .unwrap_or_default();
    if points.len() < 2 {
        return Err("Region needs at least 2 points — draw it in the node config.".into());
    }
    let mut o = PortValues::new();
    o.insert(
        "region".into(),
        PortValue::Region(crate::workflows::domain::RegionValue { camera: id, name, points }),
    );
    Ok(o)
}

/// The tracker's live "state of the world" for a camera (card: edge tracking) — every
/// currently-confirmed track across every job watching it, `GET {edge_api}/tracks/{ip}`. The
/// query counterpart to the `object_entered`/`object_exited`/`line_crossed` *events*: "what's
/// tracked right now" instead of "something just changed". Same edge-API path as
/// Start/StopRecording (tailnet, port 8080) — see `resolve_edge_api_base`.
pub(super) fn run_get_tracked_objects(inputs: &PortValues, sys: &ToolContext) -> Result<PortValues, String> {
    use crate::workflows::domain::{BBox, TrackedObjectInfo};
    use super::recording::{edge_agent, resolve_edge_api_base};

    let camera_ip = inputs
        .get("camera")
        .and_then(PortValue::as_camera)
        .map(|c| c.ip.clone())
        .ok_or("missing required input 'camera'")?;
    let base = resolve_edge_api_base(sys)?;
    let resp = edge_agent()
        .get(&format!("{base}/tracks/{camera_ip}"))
        .call()
        .map_err(|e| format!("edge get_tracked_objects: {e}"))?;
    let items: Vec<Value> = resp.into_json().unwrap_or_default();
    let objects: Vec<PortValue> = items
        .into_iter()
        .filter_map(|v| {
            let uuid = v.get("uuid")?.as_str()?.to_string();
            let class = v.get("class")?.as_str()?.to_string();
            let nums: Vec<i64> = v.get("bbox")?.as_array()?.iter().filter_map(|n| n.as_i64()).collect();
            let bbox = BBox::from_xyxy_slice(&nums)?;
            let zone = v.get("zone").and_then(|z| z.as_str()).unwrap_or("").to_string();
            Some(PortValue::TrackedObject(TrackedObjectInfo { uuid, class, bbox, zone }))
        })
        .collect();
    let mut o = PortValues::new();
    o.insert("count".into(), PortValue::Num(objects.len() as i64));
    o.insert("objects".into(), PortValue::List(objects));
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
