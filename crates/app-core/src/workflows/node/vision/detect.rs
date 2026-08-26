//! Finding things in a frame, and taking apart what was found.
//!
//! Two halves. The detectors run a model over an image and give back a list. The `Break` nodes
//! take one item off that list and spread it across typed ports, which is what lets the rest of a
//! graph work with a confidence or a box without knowing anything about detection.
//!
//! Detection runs LOCAL, through tract — not a sidecar and not a cloud call. A frame that has to
//! leave the machine to be looked at is a frame that arrives too late to act on.

use super::super::{device_from_camera, jpeg_dimensions, PortValue, PortValues};
use super::*;
use crate::agent::tools::ToolContext;
use serde_json::Value;

pub(crate) fn run_person_detection(
    inputs: &PortValues,
    sys: &ToolContext,
) -> Result<PortValues, String> {
    let bytes = inputs
        .get("image")
        .and_then(PortValue::as_image)
        .ok_or("missing required input 'image'")?;
    let people = sys.vision.detect_persons_detailed(bytes)?;
    let count = people.len() as i64;
    let confidence = people.iter().map(|p| p.confidence).fold(0.0_f32, f32::max);
    let persons: Vec<PortValue> = people.into_iter().map(PortValue::Person).collect();
    let (w, h) = jpeg_dimensions(bytes);
    let mut o = PortValues::new();
    o.insert("count".into(), PortValue::Num(count));
    o.insert(
        "confidence".into(),
        PortValue::Num((confidence * 100.0).round() as i64),
    );
    o.insert("persons".into(), PortValue::List(persons));
    o.insert("width".into(), PortValue::Num(w));
    o.insert("height".into(), PortValue::Num(h));
    o.insert("ok".into(), PortValue::Bool(true));
    Ok(o)
}

pub(crate) fn run_object_detection(
    config: &Value,
    inputs: &PortValues,
    sys: &ToolContext,
) -> Result<PortValues, String> {
    let bytes = inputs
        .get("image")
        .and_then(PortValue::as_image)
        .ok_or("missing required input 'image'")?;
    let class_name = inputs
        .get("class")
        .and_then(PortValue::as_text)
        .or_else(|| config.get("class").and_then(|v| v.as_str()))
        .unwrap_or("person");
    let class_id = crate::agent::detector::class_id_for_name(class_name)
        .ok_or_else(|| format!("unknown COCO class: '{class_name}'"))?;
    let detections = sys.vision.detect_objects_detailed(bytes, class_id)?;
    let count = detections.len() as i64;
    let confidence = detections
        .iter()
        .map(|d| d.confidence)
        .fold(0.0_f32, f32::max);
    let items: Vec<PortValue> = detections.into_iter().map(PortValue::Person).collect();
    let (w, h) = jpeg_dimensions(bytes);
    let mut o = PortValues::new();
    o.insert("count".into(), PortValue::Num(count));
    o.insert(
        "confidence".into(),
        PortValue::Num((confidence * 100.0).round() as i64),
    );
    o.insert("detections".into(), PortValue::List(items));
    o.insert("width".into(), PortValue::Num(w));
    o.insert("height".into(), PortValue::Num(h));
    o.insert("ok".into(), PortValue::Bool(true));
    Ok(o)
}

/// Destructure a PersonDetection element → bbox + confidence.
/// Destructure a `TrackedObject` element (from `GetTrackedObjects`' `objects` array) into its
/// fields — same Unreal-style "Break" shape as `run_break_person_detection`.
pub(crate) fn run_break_tracked_object(inputs: &PortValues) -> Result<PortValues, String> {
    let t = inputs
        .get("object")
        .and_then(PortValue::as_tracked_object)
        .ok_or("missing required input 'object'")?;
    let mut o = PortValues::new();
    o.insert("uuid".into(), PortValue::Text(t.uuid.clone()));
    o.insert("class".into(), PortValue::Text(t.class.clone()));
    o.insert("bbox".into(), PortValue::Bbox(t.bbox));
    o.insert("zone".into(), PortValue::Text(t.zone.clone()));
    Ok(o)
}

pub(crate) fn run_break_person_detection(inputs: &PortValues) -> Result<PortValues, String> {
    let p = inputs
        .get("person")
        .and_then(PortValue::as_person)
        .ok_or("missing required input 'person'")?;
    let mut o = PortValues::new();
    o.insert("bbox".into(), PortValue::Bbox(bbox_from_xyxy(&p.bbox)));
    o.insert(
        "confidence".into(),
        PortValue::Num((p.confidence * 100.0).round() as i64),
    );
    Ok(o)
}

/// Destructure a generic Detection element (from Detect Objects) → bbox + confidence.
pub(crate) fn run_break_detection(inputs: &PortValues) -> Result<PortValues, String> {
    let d = inputs
        .get("detection")
        .and_then(PortValue::as_person)
        .ok_or("missing required input 'detection'")?;
    let mut o = PortValues::new();
    o.insert("bbox".into(), PortValue::Bbox(bbox_from_xyxy(&d.bbox)));
    o.insert(
        "confidence".into(),
        PortValue::Num((d.confidence * 100.0).round() as i64),
    );
    Ok(o)
}

/// Destructure a FaceDetection element → bbox + confidence + landmarks.
pub(crate) fn run_break_face_detection(inputs: &PortValues) -> Result<PortValues, String> {
    let f = inputs
        .get("face")
        .and_then(PortValue::as_face)
        .ok_or("missing required input 'face'")?;
    let mut o = PortValues::new();
    o.insert("bbox".into(), PortValue::Bbox(bbox_from_xyxy(&f.bbox)));
    o.insert(
        "confidence".into(),
        PortValue::Num((f.confidence * 100.0).round() as i64),
    );
    o.insert("landmarks".into(), PortValue::Num(f.landmarks.len() as i64));
    Ok(o)
}
