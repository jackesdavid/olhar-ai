//! Whether anything moved between two frames.
//!
//! Cheap on purpose: it is the gate that decides whether the expensive work — detection,
//! recognition — is worth doing at all, so it has to cost far less than what it guards.

use super::super::{device_from_camera, jpeg_dimensions, PortValue, PortValues};
use super::*;
use crate::agent::tools::ToolContext;
use serde_json::Value;

pub(crate) fn run_motion_detection(
    config: &Value,
    inputs: &PortValues,
) -> Result<PortValues, String> {
    let a = inputs
        .get("image_a")
        .and_then(PortValue::as_image)
        .ok_or("missing required input 'image_a'")?;
    let b = inputs
        .get("image_b")
        .and_then(PortValue::as_image)
        .ok_or("missing required input 'image_b'")?;
    let pixel_threshold = cfg_i64(config, "pixel_threshold", 25).clamp(0, 255) as i16;
    let sensitivity = cfg_i64(config, "sensitivity", 2).clamp(0, 100) as f64;
    let score = motion_score(a, b, pixel_threshold)?;
    let mut o = PortValues::new();
    o.insert("moved".into(), PortValue::Bool(score >= sensitivity));
    o.insert("score".into(), PortValue::Num(score.round() as i64));
    o.insert("ok".into(), PortValue::Bool(true));
    Ok(o)
}

/// Camera-bound motion detector: resolves its own camera, grabs two frames apart, and reports
/// moved/score. Self-contained per run — drop in a Cron graph to watch a feed continuously.
pub(crate) fn run_motion_watch(
    config: &Value,
    inputs: &PortValues,
    sys: &ToolContext,
) -> Result<PortValues, String> {
    let cam = inputs
        .get("camera")
        .and_then(PortValue::as_camera)
        .ok_or("missing required camera input")?;
    let device = {
        let devices = sys.devices.lock().unwrap();
        device_from_camera(cam, &devices)?.clone()
    };
    let pixel_threshold = cfg_i64(config, "pixel_threshold", 25).clamp(0, 255) as i16;
    let sensitivity = cfg_i64(config, "sensitivity", 2).clamp(0, 100) as f64;
    let gap_ms = cfg_i64(config, "sample_gap_ms", 400).clamp(0, 5000) as u64;
    let a = sys.frames.snapshot(&device)?;
    std::thread::sleep(std::time::Duration::from_millis(gap_ms));
    let b = sys.frames.snapshot(&device)?;
    let score = motion_score(&a, &b, pixel_threshold)?;
    let mut o = PortValues::new();
    o.insert("moved".into(), PortValue::Bool(score >= sensitivity));
    o.insert("score".into(), PortValue::Num(score.round() as i64));
    o.insert("ok".into(), PortValue::Bool(true));
    Ok(o)
}
