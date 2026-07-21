//! Edge face detection (#117 Phase 5) — **YuNet** (libfacedetection, Shiqi Yu, MIT), a tiny
//! (~230 KB) fixed-input face detector run locally via `tract` — **pure Rust, NO OpenCV** — exactly
//! like [`super::detector`]'s YOLOX person detector. Fixed `[1,3,640,640]` input, standard ONNX ops
//! only (tract-friendly, proven by the `yunet_loads_and_runs` test). Outputs per stride {8,16,32}:
//! `cls`/`obj` (g·g), `bbox` (g·g·4), `kps` (g·g·10). We decode `score = √(cls·obj)`,
//! `box = (grid + Δ)·stride` (exp for w/h), NMS, and map back to source pixels — YuNet's own
//! anchor-free decode, reimplemented here in Rust (not a call into any library).
use std::sync::{Arc, OnceLock};

use image::imageops::FilterType;
use tract_onnx::prelude::tract_ndarray::Array4;
use tract_onnx::prelude::*;

static MODEL: &[u8] = include_bytes!("models/yunet_2023mar.onnx");
const INPUT: usize = 640;
const STRIDES: [usize; 3] = [8, 16, 32];
const CONF_THRESH: f32 = 0.6;
const NMS_THRESH: f32 = 0.3;

/// One detected face, in **source-image pixels**.
#[derive(Debug, Clone, Copy)]
pub struct FaceBox {
    /// `[x1, y1, x2, y2]` in source pixels.
    pub xyxy: [f32; 4],
    pub score: f32,
    /// The 5 facial landmarks (right eye, left eye, nose, right mouth, left mouth) in source
    /// pixels — YuNet's `kps` output, decoded here. Threaded to the sidecar so ArcFace aligns from
    /// these instead of re-detecting (SCRFD) inside the crop (card #122). `[[0,0];5]` if unavailable.
    pub kps: [[f32; 2]; 5],
}

impl FaceBox {
    pub fn area(&self) -> f32 {
        (self.xyxy[2] - self.xyxy[0]).max(0.0) * (self.xyxy[3] - self.xyxy[1]).max(0.0)
    }
}

/// Detect faces in a JPEG/PNG frame → source-pixel boxes above [`CONF_THRESH`], post-NMS.
pub fn detect(image_bytes: &[u8]) -> Result<Vec<FaceBox>, String> {
    let (input, ratio) = preprocess(image_bytes)?;
    run_and_decode(input, ratio)
}

/// [`detect`] over the frame hub's **already-decoded** packed BGR buffer (`w*h*3`) — no second JPEG
/// decode (the edge hook reuses the same frame motion saw).
pub fn detect_bgr(bgr: &[u8], w: u32, h: u32) -> Result<Vec<FaceBox>, String> {
    let (input, ratio) = preprocess_bgr(bgr, w, h)?;
    run_and_decode(input, ratio)
}

/// Run the model on a prepared `[1,3,640,640]` tensor and decode the 12 per-stride outputs → NMS'd
/// source-pixel boxes.
fn run_and_decode(input: Array4<f32>, ratio: f32) -> Result<Vec<FaceBox>, String> {
    let tensor: Tensor = input.into();
    let out = model()?
        .run(tvec!(tensor.into()))
        .map_err(|e| format!("yunet: inference: {e}"))?;
    if out.len() != 12 {
        return Err(format!("yunet: expected 12 outputs, got {}", out.len()));
    }
    // Read a plain f32 slice from output `i` (graph order: cls_{8,16,32}, obj_…, bbox_…, kps_…).
    let read = |i: usize| -> Result<Vec<f32>, String> {
        let v = out[i]
            .to_plain_array_view::<f32>()
            .map_err(|e| format!("yunet: read output {i}: {e}"))?;
        Ok(v.as_slice().map(<[f32]>::to_vec).unwrap_or_else(|| v.iter().copied().collect()))
    };

    let mut dets: Vec<FaceBox> = Vec::new();
    for (si, &stride) in STRIDES.iter().enumerate() {
        // YuNet decode. cls/obj are interchangeable here — score = √(cls·obj) is symmetric — so a
        // cls/obj swap in the output order is harmless; only bbox (index 6+si) must be right, and it
        // is by the ONNX graph's output order.
        let cls = read(si)?;
        let obj = read(3 + si)?;
        let bbox = read(6 + si)?;
        let kps = read(9 + si)?;
        let g = INPUT / stride;
        for idx in 0..(g * g) {
            if idx >= cls.len() || idx >= obj.len() {
                break;
            }
            let score = (cls[idx].clamp(0.0, 1.0) * obj[idx].clamp(0.0, 1.0)).sqrt();
            if score < CONF_THRESH {
                continue;
            }
            let (col, row) = ((idx % g) as f32, (idx / g) as f32);
            let b = idx * 4;
            if b + 4 > bbox.len() {
                break;
            }
            let s = stride as f32;
            let cx = (col + bbox[b]) * s;
            let cy = (row + bbox[b + 1]) * s;
            let w = bbox[b + 2].exp() * s;
            let h = bbox[b + 3].exp() * s;
            // 5 landmarks: same anchor-relative decode as the box centre (linear, no exp), mapped
            // back to source pixels. `k + 10 > kps.len()` guards a short/ragged output.
            let k = idx * 10;
            let mut lmk = [[0.0f32; 2]; 5];
            if k + 10 <= kps.len() {
                for (p, pt) in lmk.iter_mut().enumerate() {
                    *pt = [
                        (col + kps[k + 2 * p]) * s / ratio,
                        (row + kps[k + 2 * p + 1]) * s / ratio,
                    ];
                }
            }
            dets.push(FaceBox {
                score,
                xyxy: [
                    (cx - w * 0.5) / ratio,
                    (cy - h * 0.5) / ratio,
                    (cx + w * 0.5) / ratio,
                    (cy + h * 0.5) / ratio,
                ],
                kps: lmk,
            });
        }
    }
    Ok(nms(dets, NMS_THRESH))
}

fn model() -> Result<&'static Arc<TypedRunnableModel>, String> {
    static PLAN: OnceLock<Result<Arc<TypedRunnableModel>, String>> = OnceLock::new();
    PLAN.get_or_init(build_model).as_ref().map_err(Clone::clone)
}

fn build_model() -> Result<Arc<TypedRunnableModel>, String> {
    let mut cursor = std::io::Cursor::new(MODEL);
    let plan = tract_onnx::onnx()
        .model_for_read(&mut cursor)
        .map_err(|e| format!("yunet: parse ONNX: {e}"))?
        .with_input_fact(0, InferenceFact::dt_shape(f32::datum_type(), tvec!(1, 3, INPUT, INPUT)))
        .map_err(|e| format!("yunet: set input fact: {e}"))?
        .into_optimized()
        .map_err(|e| format!("yunet: optimize: {e}"))?
        .into_runnable()
        .map_err(|e| format!("yunet: runnable: {e}"))?;
    Ok(plan)
}

/// Source byte order of a decoded 3-channel image. The tensor is always written **BGR** (YuNet, like
/// YOLOX, takes BGR with no `/255`); this only says how to read the source pixel.
#[derive(Clone, Copy)]
enum PixelOrder {
    Rgb,
    Bgr,
}

fn preprocess(image_bytes: &[u8]) -> Result<(Array4<f32>, f32), String> {
    let img = image::load_from_memory(image_bytes)
        .map_err(|e| format!("yunet: decode frame: {e}"))?
        .to_rgb8();
    letterbox(&img, PixelOrder::Rgb)
}

/// [`preprocess`] over packed 8-bit BGR pixels (`w*h*3`). `image` has no BGR8 type, but resizing is
/// channel-agnostic, so we carry BGR bytes through an `RgbImage` and read them back BGR.
fn preprocess_bgr(bgr: &[u8], w: u32, h: u32) -> Result<(Array4<f32>, f32), String> {
    let want = (w as usize) * (h as usize) * 3;
    if want == 0 || bgr.len() < want {
        return Err(format!("yunet: bgr buffer is {} bytes, want {want} for {w}x{h}", bgr.len()));
    }
    let img: image::RgbImage = image::ImageBuffer::from_raw(w, h, bgr[..want].to_vec())
        .ok_or_else(|| "yunet: bgr buffer does not match w*h*3".to_string())?;
    letterbox(&img, PixelOrder::Bgr)
}

/// Letterbox an 8-bit 3-channel image into a `[1,3,640,640]` BGR, raw 0–255 CHW tensor (aspect
/// preserved, black-padded bottom/right). Returns the tensor + resize ratio (`source_px = px/ratio`).
fn letterbox(img: &image::RgbImage, order: PixelOrder) -> Result<(Array4<f32>, f32), String> {
    let (w0, h0) = (img.width() as f32, img.height() as f32);
    if w0 == 0.0 || h0 == 0.0 {
        return Err("yunet: empty frame".into());
    }
    let ratio = (INPUT as f32 / w0).min(INPUT as f32 / h0);
    let (nw, nh) = ((w0 * ratio).round() as u32, (h0 * ratio).round() as u32);
    let resized = image::imageops::resize(img, nw.max(1), nh.max(1), FilterType::Triangle);
    let mut arr = Array4::<f32>::zeros((1, 3, INPUT, INPUT));
    for y in 0..resized.height() as usize {
        for x in 0..resized.width() as usize {
            let px = resized.get_pixel(x as u32, y as u32).0;
            let [b, g, r] = match order {
                PixelOrder::Rgb => [px[2], px[1], px[0]],
                PixelOrder::Bgr => px,
            };
            arr[[0, 0, y, x]] = b as f32;
            arr[[0, 1, y, x]] = g as f32;
            arr[[0, 2, y, x]] = r as f32;
        }
    }
    Ok((arr, ratio))
}

/// Greedy non-max suppression, highest score first.
fn nms(mut dets: Vec<FaceBox>, iou_thresh: f32) -> Vec<FaceBox> {
    dets.sort_by(|a, b| b.score.total_cmp(&a.score));
    let mut keep: Vec<FaceBox> = Vec::new();
    'candidate: for d in dets {
        for k in &keep {
            if iou(k.xyxy, d.xyxy) > iou_thresh {
                continue 'candidate;
            }
        }
        keep.push(d);
    }
    keep
}

fn iou(a: [f32; 4], b: [f32; 4]) -> f32 {
    let x1 = a[0].max(b[0]);
    let y1 = a[1].max(b[1]);
    let x2 = a[2].min(b[2]);
    let y2 = a[3].min(b[3]);
    let inter = (x2 - x1).max(0.0) * (y2 - y1).max(0.0);
    let area_a = (a[2] - a[0]).max(0.0) * (a[3] - a[1]).max(0.0);
    let area_b = (b[2] - b[0]).max(0.0) * (b[3] - b[1]).max(0.0);
    let union = area_a + area_b - inter;
    if union <= 0.0 { 0.0 } else { inter / union }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// tract must load + run YuNet (fixed shape, standard ops).
    #[test]
    fn yunet_loads_and_runs() {
        let blank = Array4::<f32>::zeros((1, 3, INPUT, INPUT));
        let out = model().unwrap().run(tvec!(Tensor::from(blank).into())).unwrap();
        assert_eq!(out.len(), 12, "YuNet has 12 outputs");
    }

    /// A black frame yields no faces (decode + threshold sanity — no false positives).
    #[test]
    fn black_frame_has_no_faces() {
        let img = image::RgbImage::from_pixel(64, 64, image::Rgb([0, 0, 0]));
        let mut buf = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(img)
            .write_to(&mut buf, image::ImageFormat::Jpeg)
            .unwrap();
        let faces = detect(buf.get_ref()).unwrap();
        assert!(faces.is_empty(), "a black frame must have no faces, got {}", faces.len());
    }
}
