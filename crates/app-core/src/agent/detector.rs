//! Native person/object detection with **YOLOX-Nano** (Apache-2.0), run in-process
//! via `tract` — a pure-Rust ONNX engine. This replaces the old Python/RunPod vision
//! sidecar: no network round-trip, no cold start, no native ONNX Runtime to compile
//! or bundle, and a license clean enough to ship in a distributed desktop app (unlike
//! the AGPL YOLOv8 it supersedes).
//!
//! The model (`models/yolox_nano.onnx`, ≈3.6 MB) is embedded in the binary, so
//! detection works fully offline with no install step. The pipeline mirrors YOLOX's
//! reference `preproc` + `demo_postprocess` exactly — getting any of these wrong
//! silently degrades accuracy rather than erroring:
//!
//! * **Preprocess** — letterbox to 416×416 preserving aspect (pad bottom/right with
//!   114), **BGR** channel order, **raw 0–255 floats** (YOLOX does *no* `/255` and
//!   *no* mean/std normalisation), HWC→CHW.
//! * **Postprocess** — the stock export emits `[1,3549,85]` *undecoded* box geometry
//!   over strides 8/16/32 (obj/cls are already sigmoid-activated in-graph). We decode
//!   `xy=(raw+grid)*stride`, `wh=exp(raw)*stride`, score `= obj*cls`, NMS, then divide
//!   by the letterbox ratio to land back in source-image pixels.
//!
//! tract's runnable plan is immutable and re-entrant (its `run` takes `&Arc<Self>`),
//! so the model is a process-lifetime `Arc<TypedRunnableModel>` shared across calls
//! with no locking.

use std::sync::{Arc, OnceLock};

use image::imageops::FilterType;
use tract_onnx::prelude::tract_ndarray::Array4;
use tract_onnx::prelude::*;

/// Embedded YOLOX-Nano weights. Apache-2.0 — see `models/LICENSE.yolox`.
static MODEL: &[u8] = include_bytes!("models/yolox_nano.onnx");

const INPUT: usize = 416; // YOLOX-Nano/Tiny train + export size
const PAD: f32 = 114.0; // YOLOX letterbox fill
const STRIDES: [usize; 3] = [8, 16, 32];
const NUM_CLASSES: usize = 80; // COCO
const PERSON_CLASS: u32 = 0; // COCO class 0 = "person"
const CONF_THRESH: f32 = 0.30; // obj*cls score floor
const IOU_THRESH: f32 = 0.45; // NMS overlap

/// One detected object, in **source-image** pixel coordinates (`[x1,y1,x2,y2]`).
#[derive(Debug, Clone, Copy)]
pub struct Detection {
    pub class: u32,
    pub score: f32,
    pub xyxy: [f32; 4],
}

/// Person-detection summary, shaped for the workflow node / agent tool.
#[derive(Debug, Clone, Copy)]
pub struct PersonResult {
    pub person: bool,
    pub count: i64,
    /// Highest person score, `0.0..=1.0`.
    pub confidence: f32,
}

/// One detected person: its source-pixel box `[x1,y1,x2,y2]` + detection confidence.
/// The element type of a `PersonDetection[]` (workflow `PersonArray` port).
#[derive(Debug, Clone, Copy)]
pub struct PersonDetection {
    pub bbox: [f32; 4],
    pub confidence: f32,
}

/// Detect every COCO object in a JPEG/PNG frame, post-NMS, in source pixels.
pub fn detect(image_bytes: &[u8]) -> Result<Vec<Detection>, String> {
    let (input, ratio) = preprocess(image_bytes)?;
    let raw = infer(input)?;
    Ok(nms(decode(&raw, ratio), IOU_THRESH))
}

/// The `(width, height)` of a JPEG/PNG frame in source pixels — for scaling normalized
/// [`crate::tracking::Line`] coordinates to the same pixel space `Detection::xyxy` is in.
pub fn dimensions(image_bytes: &[u8]) -> Result<(u32, u32), String> {
    let img = image::load_from_memory(image_bytes)
        .map_err(|e| format!("detector: decode frame: {e}"))?;
    Ok((img.width(), img.height()))
}

/// Detect people only — `person`/`count`/`confidence` for the PersonDetection node.
pub fn detect_person(image_bytes: &[u8]) -> Result<PersonResult, String> {
    let people: Vec<Detection> =
        detect(image_bytes)?.into_iter().filter(|d| d.class == PERSON_CLASS).collect();
    let confidence = people.iter().map(|d| d.score).fold(0.0_f32, f32::max);
    Ok(PersonResult { person: !people.is_empty(), count: people.len() as i64, confidence })
}

/// Detect people and return one [`PersonDetection`] per person (box + confidence) — the
/// `PersonDetection[]` the workflow node emits.
pub fn detect_persons_detailed(image_bytes: &[u8]) -> Result<Vec<PersonDetection>, String> {
    Ok(detect(image_bytes)?
        .into_iter()
        .filter(|d| d.class == PERSON_CLASS)
        .map(|d| PersonDetection { bbox: d.xyxy, confidence: d.score })
        .collect())
}

/// All 80 COCO class names in class-index order (index 0 = "person", etc.).
pub const COCO_CLASSES: [&str; 80] = [
    "person", "bicycle", "car", "motorcycle", "airplane", "bus", "train", "truck", "boat",
    "traffic light", "fire hydrant", "stop sign", "parking meter", "bench", "bird", "cat",
    "dog", "horse", "sheep", "cow", "elephant", "bear", "zebra", "giraffe", "backpack",
    "umbrella", "handbag", "tie", "suitcase", "frisbee", "skis", "snowboard", "sports ball",
    "kite", "baseball bat", "baseball glove", "skateboard", "surfboard", "tennis racket",
    "bottle", "wine glass", "cup", "fork", "knife", "spoon", "bowl", "banana", "apple",
    "sandwich", "orange", "broccoli", "carrot", "hot dog", "pizza", "donut", "cake",
    "chair", "couch", "potted plant", "bed", "dining table", "toilet", "tv", "laptop",
    "mouse", "remote", "keyboard", "cell phone", "microwave", "oven", "toaster", "sink",
    "refrigerator", "book", "clock", "vase", "scissors", "teddy bear", "hair drier",
    "toothbrush",
];

/// Resolve a COCO class name (case-insensitive) to its class index, or `None` if unknown.
pub fn class_id_for_name(name: &str) -> Option<u32> {
    COCO_CLASSES.iter().position(|&c| c.eq_ignore_ascii_case(name)).map(|i| i as u32)
}

/// Detect objects of a specific COCO class (by class id) and return one [`PersonDetection`]
/// per detection (box + confidence) — the generic counterpart to `detect_persons_detailed`.
pub fn detect_objects_detailed(
    image_bytes: &[u8],
    class_id: u32,
) -> Result<Vec<PersonDetection>, String> {
    Ok(detect(image_bytes)?
        .into_iter()
        .filter(|d| d.class == class_id)
        .map(|d| PersonDetection { bbox: d.xyxy, confidence: d.score })
        .collect())
}

/// Process-lifetime YOLOX plan, built once on first use. The build result (ok or the
/// error string) is cached, so a load failure is reported on every call without
/// re-attempting a doomed init. The returned `&Arc` is exactly what `run` wants.
fn model() -> Result<&'static Arc<TypedRunnableModel>, String> {
    static MODEL_PLAN: OnceLock<Result<Arc<TypedRunnableModel>, String>> = OnceLock::new();
    MODEL_PLAN.get_or_init(build_model).as_ref().map_err(Clone::clone)
}

fn build_model() -> Result<Arc<TypedRunnableModel>, String> {
    let mut cursor = std::io::Cursor::new(MODEL);
    let plan = tract_onnx::onnx()
        .model_for_read(&mut cursor)
        .map_err(|e| format!("tract: parse YOLOX ONNX: {e}"))?
        // Pin the input shape so tract can fully optimize the graph.
        .with_input_fact(0, InferenceFact::dt_shape(f32::datum_type(), tvec!(1, 3, INPUT, INPUT)))
        .map_err(|e| format!("tract: set input fact: {e}"))?
        .into_optimized()
        .map_err(|e| format!("tract: optimize graph: {e}"))?
        .into_runnable()
        .map_err(|e| format!("tract: build runnable plan: {e}"))?;
    Ok(plan)
}

/// Letterbox `image_bytes` into a `[1,3,416,416]` BGR, raw-0..255 CHW tensor.
/// Returns the tensor and the resize ratio `r` (`source_px = letterbox_px / r`).
fn preprocess(image_bytes: &[u8]) -> Result<(Array4<f32>, f32), String> {
    let img = image::load_from_memory(image_bytes)
        .map_err(|e| format!("detector: decode frame: {e}"))?
        .to_rgb8();
    let (w0, h0) = (img.width() as f32, img.height() as f32);
    if w0 == 0.0 || h0 == 0.0 {
        return Err("detector: empty frame".into());
    }
    let ratio = (INPUT as f32 / w0).min(INPUT as f32 / h0);
    let (nw, nh) = ((w0 * ratio).round() as u32, (h0 * ratio).round() as u32);
    let resized = image::imageops::resize(&img, nw.max(1), nh.max(1), FilterType::Triangle);

    // Pad-filled canvas, resized frame pinned to the top-left, channels in BGR order
    // (YOLOX trains on cv2-decoded BGR with no RGB swap).
    let mut arr = Array4::<f32>::from_elem((1, 3, INPUT, INPUT), PAD);
    for y in 0..resized.height() as usize {
        for x in 0..resized.width() as usize {
            let [r, g, b] = resized.get_pixel(x as u32, y as u32).0;
            arr[[0, 0, y, x]] = b as f32;
            arr[[0, 1, y, x]] = g as f32;
            arr[[0, 2, y, x]] = r as f32;
        }
    }
    Ok((arr, ratio))
}

/// Run the model, returning the flat `[1,3549,85]` output (`3549` anchors × `85`).
fn infer(input: Array4<f32>) -> Result<Vec<f32>, String> {
    let tensor: Tensor = input.into();
    let result =
        model()?.run(tvec!(tensor.into())).map_err(|e| format!("tract: inference: {e}"))?;
    let view =
        result[0].to_plain_array_view::<f32>().map_err(|e| format!("tract: read output: {e}"))?;
    Ok(view.as_slice().map(<[f32]>::to_vec).unwrap_or_else(|| view.iter().copied().collect()))
}

const STEP: usize = 5 + NUM_CLASSES; // per-anchor: cx,cy,w,h,obj + 80 class scores

/// Decode raw YOLOX output into source-pixel boxes above [`CONF_THRESH`], pre-NMS.
fn decode(raw: &[f32], ratio: f32) -> Vec<Detection> {
    let mut out = Vec::new();
    let mut a = 0usize; // anchor index, walked in stride 8 → 16 → 32 order
    for &stride in &STRIDES {
        let grid = INPUT / stride;
        for gy in 0..grid {
            for gx in 0..grid {
                let base = a * STEP;
                a += 1;
                if base + STEP > raw.len() {
                    return out; // malformed output; bail with what we have
                }
                let cx = (raw[base] + gx as f32) * stride as f32;
                let cy = (raw[base + 1] + gy as f32) * stride as f32;
                let w = raw[base + 2].exp() * stride as f32;
                let h = raw[base + 3].exp() * stride as f32;
                let obj = raw[base + 4];

                let (mut best_c, mut best_p) = (0u32, 0.0f32);
                for c in 0..NUM_CLASSES {
                    let p = raw[base + 5 + c];
                    if p > best_p {
                        best_p = p;
                        best_c = c as u32;
                    }
                }
                let score = obj * best_p;
                if score < CONF_THRESH {
                    continue;
                }
                out.push(Detection {
                    class: best_c,
                    score,
                    xyxy: [
                        (cx - w * 0.5) / ratio,
                        (cy - h * 0.5) / ratio,
                        (cx + w * 0.5) / ratio,
                        (cy + h * 0.5) / ratio,
                    ],
                });
            }
        }
    }
    out
}

/// Greedy per-class non-max suppression, highest score first.
fn nms(mut dets: Vec<Detection>, iou_thresh: f32) -> Vec<Detection> {
    dets.sort_by(|a, b| b.score.total_cmp(&a.score));
    let mut keep: Vec<Detection> = Vec::new();
    'candidate: for d in dets {
        for k in &keep {
            if k.class == d.class && iou(k.xyxy, d.xyxy) > iou_thresh {
                continue 'candidate;
            }
        }
        keep.push(d);
    }
    keep
}

/// Intersection-over-union of two `[x1,y1,x2,y2]` boxes.
fn iou(a: [f32; 4], b: [f32; 4]) -> f32 {
    let x1 = a[0].max(b[0]);
    let y1 = a[1].max(b[1]);
    let x2 = a[2].min(b[2]);
    let y2 = a[3].min(b[3]);
    let inter = (x2 - x1).max(0.0) * (y2 - y1).max(0.0);
    let area_a = (a[2] - a[0]).max(0.0) * (a[3] - a[1]).max(0.0);
    let area_b = (b[2] - b[0]).max(0.0) * (b[3] - b[1]).max(0.0);
    let union = area_a + area_b - inter;
    if union <= 0.0 {
        0.0
    } else {
        inter / union
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iou_identical_is_one() {
        assert!((iou([0.0, 0.0, 10.0, 10.0], [0.0, 0.0, 10.0, 10.0]) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn iou_disjoint_is_zero() {
        assert_eq!(iou([0.0, 0.0, 10.0, 10.0], [20.0, 20.0, 30.0, 30.0]), 0.0);
    }

    #[test]
    fn iou_half_overlap() {
        // Two 10×10 boxes overlapping in a 10×5 strip: inter=50, union=150.
        let got = iou([0.0, 0.0, 10.0, 10.0], [0.0, 5.0, 10.0, 15.0]);
        assert!((got - 50.0 / 150.0).abs() < 1e-6);
    }

    #[test]
    fn nms_drops_overlapping_same_class() {
        let dets = vec![
            Detection { class: 0, score: 0.9, xyxy: [0.0, 0.0, 10.0, 10.0] },
            Detection { class: 0, score: 0.8, xyxy: [1.0, 1.0, 11.0, 11.0] }, // ~0.68 IoU → dropped
            Detection { class: 0, score: 0.7, xyxy: [50.0, 50.0, 60.0, 60.0] }, // disjoint → kept
        ];
        let kept = nms(dets, IOU_THRESH);
        assert_eq!(kept.len(), 2);
        assert_eq!(kept[0].score, 0.9);
    }

    #[test]
    fn nms_keeps_overlapping_different_class() {
        let dets = vec![
            Detection { class: 0, score: 0.9, xyxy: [0.0, 0.0, 10.0, 10.0] },
            Detection { class: 2, score: 0.8, xyxy: [0.0, 0.0, 10.0, 10.0] }, // same box, other class
        ];
        assert_eq!(nms(dets, IOU_THRESH).len(), 2);
    }

    #[test]
    fn decode_walks_3549_anchors() {
        // 52² + 26² + 13² over strides 8/16/32 at 416 px.
        let expected: usize = STRIDES.iter().map(|s| (INPUT / s).pow(2)).sum();
        assert_eq!(expected, 3549);
        // All-zero objectness ⇒ no detections survive the threshold.
        let raw = vec![0.0f32; expected * STEP];
        assert!(decode(&raw, 1.0).is_empty());
    }

    #[test]
    fn decode_recovers_a_planted_box() {
        let n: usize = STRIDES.iter().map(|s| (INPUT / s).pow(2)).sum();
        let mut raw = vec![0.0f32; n * STEP];
        // Plant a confident person at the very first anchor (stride 8, grid 0,0).
        // raw box geometry: cx=(0+0)*8=0, cy=0, w=exp(0)*8=8, h=8 → centered box.
        let base = 0;
        raw[base + 4] = 0.9; // obj
        raw[base + 5] = 0.95; // class 0 (person)
        let dets = decode(&raw, 1.0);
        assert_eq!(dets.len(), 1);
        assert_eq!(dets[0].class, 0);
        assert!((dets[0].score - 0.9 * 0.95).abs() < 1e-6);
        // Box is centered at (0,0) with w=h=8 ⇒ [-4,-4,4,4].
        assert!((dets[0].xyxy[2] - 4.0).abs() < 1e-4);
    }
}
