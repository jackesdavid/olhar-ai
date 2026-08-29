//! What travels along a wire, and what a value of each kind looks like.
//!
//! A port has a TYPE, and the type is a plain string — `text`, `num`, `camera`. Open on purpose:
//! the engine knows nothing about cameras or faces, and a product declares whatever vocabulary its
//! nodes speak. What is fixed is that two ports connect only when their types agree, which is what
//! makes a wrong wire impossible to draw rather than a mystery at run time.
//!
//! The structs below are the shapes behind the domain-specific types — what a `camera` port
//! actually carries.

use super::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;

// ---------------------------------------------------------------------------
// Ports
// ---------------------------------------------------------------------------

/// The port types this product adds to the engine's builtin set.
///
/// [`PortType`](ggraph_core::PortType) is an open newtype over a string, so a product declares
/// its own next to the builtins and the engine moves values along wires without knowing what
/// they are. **The strings are the ones already on the wire** — they are what stored graphs
/// name, what `/workflows/catalog` returns and what the editor colours by, so every one of
/// these is a compatibility constraint rather than a naming choice.
pub mod types {
    pub use ggraph_core::PortType;

    // Builtins, re-exported under this product's names so every port declaration reads the
    // same way. Three have a wire name that differs from the engine's Rust name, and the WIRE
    // name is the one that matters: `dictionary` (engine: MAP) and `file` (engine: BYTES).
    pub const TEXT: PortType = PortType::TEXT;
    pub const NUM: PortType = PortType::NUM;
    pub const BOOL: PortType = PortType::BOOL;
    pub const IMAGE: PortType = PortType::IMAGE;
    pub const EXEC: PortType = PortType::EXEC;
    pub const ANY: PortType = PortType::ANY;
    pub const TABLE: PortType = PortType::TABLE;

    /// An arbitrary string→string dictionary (e.g. HTTP headers / form fields).
    pub const DICTIONARY: PortType = PortType::MAP;
    /// Any file (raw bytes + filename), like [`IMAGE`] but for arbitrary types.
    pub const FILE: PortType = PortType::BYTES;

    /// A notification recipient handle (carried as the recipient's label).
    pub const RECIPIENT: PortType = PortType::new_static("recipient");
    /// `String[]`.
    pub const TEXT_ARRAY: PortType = PortType::new_static("text_array");
    /// `Number[]`.
    pub const NUM_ARRAY: PortType = PortType::new_static("num_array");
    /// `PersonDetection[]` (e.g. the Person detection node's `persons` output).
    pub const PERSON_ARRAY: PortType = PortType::new_static("person_array");
    /// `FaceDetection[]`.
    pub const FACE_ARRAY: PortType = PortType::new_static("face_array");
    /// A single `PersonDetection` (e.g. a `ForEach` element).
    pub const PERSON: PortType = PortType::new_static("person");
    /// A single `FaceDetection`.
    pub const FACE: PortType = PortType::new_static("face");
    /// A face **embedding** — a fixed-length float vector (512-d ArcFace, #117). Produced by
    /// `FaceEmbedding`, consumed by `RecognizeFace`/`StoreFace`. A real type so the model/dim
    /// is the producer's concern and embedding wires can't be crossed with a plain `NUM_ARRAY`.
    pub const EMBEDDING: PortType = PortType::new_static("embedding");
    /// A bounding box — top-left/bottom-right pixel rectangle (canonical xyxy). A real type
    /// (not a bare `NUM_ARRAY`) so producers normalise at the source and consumers never guess
    /// the coordinate convention.
    pub const BBOX: PortType = PortType::new_static("bbox");
    /// The 5 facial landmarks (right/left eye, nose, right/left mouth) for ONE face, in source
    /// pixels. Produced by `OnEvent("face_detected")` (YuNet `kps`); consumed by
    /// `FaceEmbedding` to align the ArcFace crop instead of re-detecting inside it (#122).
    pub const LANDMARKS: PortType = PortType::new_static("landmarks");
    /// A camera device reference (stable device id, carried as a string on the wire).
    /// Distinct from [`TEXT`] so the editor enforces camera-to-camera wiring.
    pub const CAMERA: PortType = PortType::new_static("camera");
    /// A reusable named line/zone — 2 points is a crossing line, 3+ is a polygon region.
    pub const REGION: PortType = PortType::new_static("region");
    /// One currently-tracked entity, as reported by `GetTrackedObjects`.
    pub const TRACKED_OBJECT: PortType = PortType::new_static("tracked_object");
    /// `TrackedObject[]` — `GetTrackedObjects`' main output, iterated by `ForEach`.
    pub const TRACKED_OBJECT_ARRAY: PortType = PortType::new_static("tracked_object_array");
}

pub use ggraph_core::{Edge, Port, PortName, PortType};

/// Where the canvas was left. Stays in this product rather than coming from the engine: the
/// engine has no canvas, and the 40-pixel starting offset below is this editor's house style.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Viewport {
    pub x: f64,
    pub y: f64,
    pub zoom: f64,
}

impl Default for Viewport {
    fn default() -> Self {
        Viewport {
            x: 40.0,
            y: 40.0,
            zoom: 1.0,
        }
    }
}

/// A port declaration whose name is owned rather than static, for ports **derived from a node's
/// config** (the payload ports of `EmitEvent`/`OnEvent`, one per field of the configured event).
///
/// Now the same type as [`Port`]: the engine's `Port` carries a `PortName`, which is cheap to
/// clone and owns its data when it has to. Two types for one concept meant every caller had to
/// know which half of the API it was in.
pub type ResolvedPort = Port;

pub(super) const fn p(name: &'static str, ty: PortType, required: bool) -> Port {
    Port::new(PortName::new_static(name), ty, required)
}

/// A camera device as it appears on the wire — the safe, credential-free subset of `Device`.
/// Produced by `GetCameraByName` and `OnEvent("motion")`. Consumed by camera-input nodes and
/// `BreakCamera`. The `id` is the stable ONVIF device id (same as `Device::id`).
#[derive(Debug, Clone)]
pub struct CameraEntity {
    pub id: String,
    pub ip: String,
    pub name: String,
}

/// A reusable named line/zone, as it appears on the wire — produced by `Region`. Normalized
/// `0.0..1.0` frame coordinates, resolution-independent (same convention as the edge's
/// `app_core::tracking::Line`/`Polygon`, which this maps directly onto). Whether it means a line
/// or a zone is purely by point count, decided by whichever `OnEvent` reads it: exactly 2 points
/// is a crossing line (`line_crossed`'s `p1`/`p2`); 3+ is a polygon (`object_entered`/
/// `object_exited`'s zone) — a node that gets the wrong shape just doesn't fire, not an error.
#[derive(Debug, Clone)]
pub struct RegionValue {
    pub camera: String,
    pub name: String,
    pub points: Vec<(f32, f32)>,
}

/// One currently-tracked entity as `GetTrackedObjects` reports it — the "state of the world"
/// query counterpart to the `object_entered`/`object_exited`/`line_crossed` *events*. `zone` is
/// the name of the region it's currently inside, empty if none (either it's not in any configured
/// zone, or its job doesn't have one).
#[derive(Debug, Clone)]
pub struct TrackedObjectInfo {
    pub uuid: String,
    pub class: String,
    pub bbox: BBox,
    pub zone: String,
}

/// A bounding box in **canonical xyxy pixel coordinates**: top-left `(x1,y1)` → bottom-right
/// `(x2,y2)`. One convention everywhere — producers convert at the source (`from_xywh` for the
/// edge's motion blobs, identity for detector xyxy) so consumers (`DrawBbox`, `BlurOutsideBbox`,
/// `RecordPresence`) never need a `format` toggle. Serialised as a 4-element array `[x1,y1,x2,y2]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BBox {
    pub x1: i64,
    pub y1: i64,
    pub x2: i64,
    pub y2: i64,
}

impl BBox {
    pub fn from_xyxy(x1: i64, y1: i64, x2: i64, y2: i64) -> Self {
        Self { x1, y1, x2, y2 }
    }
    pub fn from_xywh(x: i64, y: i64, w: i64, h: i64) -> Self {
        Self {
            x1: x,
            y1: y,
            x2: x + w,
            y2: y + h,
        }
    }
    /// Parse 4 numbers (xyxy) — returns `None` if fewer than 4 are present.
    pub fn from_xyxy_slice(v: &[i64]) -> Option<Self> {
        if v.len() >= 4 {
            Some(Self {
                x1: v[0],
                y1: v[1],
                x2: v[2],
                y2: v[3],
            })
        } else {
            None
        }
    }
    pub fn to_array(self) -> [i64; 4] {
        [self.x1, self.y1, self.x2, self.y2]
    }
}

/// A node kind's ports, resolved against a configuration.
///
/// The registry needs the ports of a *kind* plus a config; `GraphNode` already knows how to
/// work them out, including the eight kinds whose ports are config-derived. Rather than keep a
/// second copy of that logic — which is exactly how the editor's palette and the scheduler came
/// to disagree about the same node — these borrow the node's own answer through a throwaway.
///
/// The config clone is paid at catalog render and at wire validation, never in a run loop.
pub fn resolved_input_ports(kind: NodeKind, config: &Value) -> Vec<ResolvedPort> {
    probe(kind, config).input_ports()
}

pub fn resolved_output_ports(kind: NodeKind, config: &Value) -> Vec<ResolvedPort> {
    probe(kind, config).output_ports()
}

pub fn resolved_exec_out_ports(kind: NodeKind, config: &Value) -> Vec<ResolvedPort> {
    let node = probe(kind, config);
    // The three switches name their arms from config; everything else has a static table.
    match kind {
        NodeKind::StringSwitch | NodeKind::AiSwitch => string_switch_arms(config)
            .into_iter()
            .chain(std::iter::once("default".to_string()))
            .map(|a| ResolvedPort::new(PortName::new(a), types::EXEC, false))
            .collect(),
        NodeKind::CameraSwitch => (1..=camera_switch_arms(config).len())
            .map(|i| format!("arm_{i}"))
            .chain(std::iter::once("default".to_string()))
            .map(|a| ResolvedPort::new(PortName::new(a), types::EXEC, false))
            .collect(),
        _ => node
            .kind
            .as_kind()
            .map(|k| k.exec_out_ports())
            .unwrap_or(&[])
            .to_vec(),
    }
}

fn probe(kind: NodeKind, config: &Value) -> GraphNode {
    GraphNode {
        id: 0,
        kind: kind.into(),
        x: 0,
        y: 0,
        config: config.clone(),
    }
}
