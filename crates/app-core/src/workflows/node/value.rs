//! What a wire actually carries.
//!
//! A typed enum, not `serde_json::Value`, and the reason is the `Image` variant: a JSON document
//! cannot hold JPEG bytes cleanly, and a graph that moves frames between nodes would spend its
//! life base64-encoding them. Everything else follows from wanting that one case to be honest.
//!
//! Each variant lines up with a port type in [`types`](crate::workflows::domain::types), which is
//! what lets a wire be refused before it is drawn rather than failing when it is run.

use super::domain::{NodeKind, Port, PortType};
use crate::agent::tools::ToolContext;
use crate::workflows::domain::types;
use serde_json::Value;
use std::collections::HashMap;

use super::*;

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
    /// An arbitrary string→string dictionary (HTTP headers / form fields).
    Dict(Vec<(String, String)>),
    /// A resolved reference to a named **Dictionary table** (#113) — carries the whole dictionary
    /// (id + schema + rows) so read nodes see its data and mutate nodes act on it by id.
    Table(crate::dictionaries::Dictionary),
    /// A face embedding — a fixed-length float vector (512-d ArcFace, #117).
    Embedding(Vec<f32>),
    /// The 5 facial landmarks for one face (source pixels). Carried by `Landmarks` ports (#122).
    Landmarks(Vec<[f32; 2]>),
    /// Any file (bytes + name + MIME).
    File(FileVal),
    /// A typed array — elements are themselves `PortValue`s. Carried by the `*Array`
    /// ports and iterated by `ForEach`.
    List(Vec<PortValue>),
    /// A detected person (element of a `PersonDetection[]`).
    Person(crate::agent::detector::PersonDetection),
    /// A detected face (element of a `FaceDetection[]`).
    Face(crate::agent::face::FaceDetection),
    /// A camera device entity (id + ip + name, credential-free). Carried by `Camera` ports.
    Camera(crate::workflows::domain::CameraEntity),
    /// A bounding box (canonical xyxy pixels). Carried by `Bbox` ports.
    Bbox(crate::workflows::domain::BBox),
    /// A reusable named line/zone (2 points = line, 3+ = polygon). Carried by `Region` ports.
    Region(crate::workflows::domain::RegionValue),
    /// One currently-tracked entity (element of `GetTrackedObjects`' `TrackedObjectArray`).
    TrackedObject(crate::workflows::domain::TrackedObjectInfo),
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
    pub fn as_dict(&self) -> Option<&[(String, String)]> {
        match self {
            PortValue::Dict(m) => Some(m),
            _ => None,
        }
    }
    pub fn as_file(&self) -> Option<&FileVal> {
        match self {
            PortValue::File(f) => Some(f),
            _ => None,
        }
    }
    pub fn as_list(&self) -> Option<&[PortValue]> {
        match self {
            PortValue::List(v) => Some(v),
            _ => None,
        }
    }
    pub fn as_person(&self) -> Option<&crate::agent::detector::PersonDetection> {
        match self {
            PortValue::Person(p) => Some(p),
            _ => None,
        }
    }
    pub fn as_face(&self) -> Option<&crate::agent::face::FaceDetection> {
        match self {
            PortValue::Face(f) => Some(f),
            _ => None,
        }
    }
    pub fn as_camera(&self) -> Option<&crate::workflows::domain::CameraEntity> {
        match self {
            PortValue::Camera(c) => Some(c),
            _ => None,
        }
    }
    pub fn as_bbox(&self) -> Option<crate::workflows::domain::BBox> {
        match self {
            PortValue::Bbox(b) => Some(*b),
            _ => None,
        }
    }
    pub fn as_embedding(&self) -> Option<&[f32]> {
        match self {
            PortValue::Embedding(v) => Some(v),
            _ => None,
        }
    }
    pub fn as_landmarks(&self) -> Option<&[[f32; 2]]> {
        match self {
            PortValue::Landmarks(v) => Some(v),
            _ => None,
        }
    }
    pub fn as_region(&self) -> Option<&crate::workflows::domain::RegionValue> {
        match self {
            PortValue::Region(r) => Some(r),
            _ => None,
        }
    }
    pub fn as_tracked_object(&self) -> Option<&crate::workflows::domain::TrackedObjectInfo> {
        match self {
            PortValue::TrackedObject(t) => Some(t),
            _ => None,
        }
    }
}

/// Read a file path into a `FileVal` (bytes + basename + guessed MIME).
pub(super) fn read_file_val(path: &str) -> Option<FileVal> {
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

pub(crate) fn parse_text_items(text: &str) -> Vec<PortValue> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    if trimmed.starts_with('[') {
        if let Ok(serde_json::Value::Array(arr)) =
            serde_json::from_str::<serde_json::Value>(trimmed)
        {
            return arr.iter().map(json_to_port_value).collect();
        }
    }
    trimmed
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| PortValue::Text(s.to_string()))
        .collect()
}

fn json_to_port_value(v: &serde_json::Value) -> PortValue {
    match v {
        serde_json::Value::String(s) => PortValue::Text(s.clone()),
        serde_json::Value::Bool(b) => PortValue::Bool(*b),
        serde_json::Value::Number(n) => match n.as_i64() {
            Some(i) => PortValue::Num(i),
            None => PortValue::Text(n.to_string()), // floats: no float PortValue → text
        },
        other => PortValue::Text(other.to_string()),
    }
}
