//! Motion detection on the edge (MOG2 adaptive background subtraction) — the *algorithm*, not the
//! threading. One [`MotionDetector`] per camera holds that camera's background model and is fed one
//! decoded frame at a time by the frame hub ([`crate::frames`]), which owns the per-camera thread
//! and the single JPEG decode every consumer shares.
//!
//! **Pure Rust** (card #79): the model is [`crate::mog2`], the morphology and blob-finding come from
//! `imageproc`, the resize from `image`. Nothing here links OpenCV — that is what lets the edge
//! cross-compile to Windows (#80) and stops the binary being soname-coupled to its build host.
//!
//! A core edge capability: the edge is the only thing near the cameras, so it owns continuous
//! motion. The hub POSTs ONE normalized motion event to control when a frame clears a small floor
//! (+ a min-interval throttle). It is graph-agnostic — per-graph sensitivity is applied in the
//! cloud (`OnEvent("motion")` → `If score >= …`), so many graphs at different sensitivities share
//! this single MOG2 pass.
//!
//! **The throttle is on the emit, not on the verdict.** [`Verdict::qualifies`] is computed for
//! every frame; only the POST to control is rate-limited. The tracker's motion gate
//! ([`crate::tracking`]) keys off the raw per-frame verdict, so it reacts immediately instead of
//! inheriting the 4s emit throttle.
//!
//! Tunables (env): `MOTION_FPS` (4), `MOTION_FLOOR_PCT` (0.5), `MOTION_MIN_INTERVAL_SECS` (4),
//! `MOTION_MIN_AREA` (300), `MOTION_PROC_W`/`MOTION_PROC_H` (480×270), `MOTION_MOG2_HISTORY` (333),
//! `MOTION_MOG2_VAR` (16).

use std::sync::Arc;
use std::time::Duration;

use base64::Engine as _;
use image::{ImageBuffer, Rgb, RgbImage};
use imageproc::contours::BorderType;
use imageproc::distance_transform::Norm;
use imageproc::point::Point;

use crate::mog2::Mog2;
use crate::state::{AppState, MotionEvent};

/// The MOG2 history is counted in **frames**, not seconds — so it has to be derived from the
/// frame rate or the background model's time constant silently changes with it. These two
/// defaults are the same ~83s of adaptation the watcher has always had (it was 500 frames at
/// 6 fps); dropping `MOTION_FPS` to 4 without rescaling history would have stretched it to ~125s.
const DEFAULT_FPS: f64 = 4.0;
const DEFAULT_HISTORY: f64 = 333.0; // ≈ DEFAULT_FPS × 83s

/// A rectangle in the processing resolution: `[x, y, w, h]`.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

pub struct Params {
    pub fps: u32,
    pub floor_pct: f64,
    pub min_interval: Duration,
    pub min_area: f64,
    pub proc_w: u32,
    pub proc_h: u32,
    pub history: i32,
    pub var_threshold: f64,
}

impl Params {
    pub fn from_env() -> Self {
        let num = |k: &str, d: f64| -> f64 {
            std::env::var(k)
                .ok()
                .and_then(|v| v.trim().parse().ok())
                .unwrap_or(d)
        };
        Params {
            fps: num("MOTION_FPS", DEFAULT_FPS).max(1.0) as u32,
            floor_pct: num("MOTION_FLOOR_PCT", 0.5),
            min_interval: Duration::from_secs(num("MOTION_MIN_INTERVAL_SECS", 4.0) as u64),
            min_area: num("MOTION_MIN_AREA", 300.0),
            proc_w: num("MOTION_PROC_W", 480.0).max(1.0) as u32,
            proc_h: num("MOTION_PROC_H", 270.0).max(1.0) as u32,
            history: num("MOTION_MOG2_HISTORY", DEFAULT_HISTORY) as i32,
            var_threshold: num("MOTION_MOG2_VAR", 16.0),
        }
    }

    /// How long the hub waits between frames — the rate every consumer of this camera samples from.
    pub fn frame_gap(&self) -> Duration {
        Duration::from_millis(1000 / u64::from(self.fps.max(1)))
    }
}

/// One frame's motion verdict, in the **processing** resolution (`proc_w`×`proc_h`).
pub struct Verdict {
    /// Cleared the floor AND has a blob above the minimum area — this frame has real motion.
    pub qualifies: bool,
    /// Foreground share of the frame, 0–100.
    pub score: f64,
    /// Largest qualifying blob's area, in processing pixels.
    pub area: f64,
    /// Number of qualifying blobs.
    pub blobs: i64,
    /// Largest qualifying blob's box, in processing pixels.
    pub rect: Rect,
}

/// One camera's background model + the scratch buffers its pipeline reuses. Fed one decoded BGR
/// frame per tick by the hub; never touches the network or the camera.
pub struct MotionDetector {
    params: Params,
    model: Mog2,
    /// The 0/255 foreground mask, at processing resolution. Reused every frame.
    mask: image::GrayImage,
    total: f64,
}

impl MotionDetector {
    pub fn new(params: Params) -> Self {
        let (w, h) = (params.proc_w, params.proc_h);
        let model = Mog2::new(w as usize, h as usize, params.history, params.var_threshold);
        MotionDetector {
            model,
            mask: image::GrayImage::new(w, h),
            total: f64::from(w * h).max(1.0),
            params,
        }
    }

    pub fn params(&self) -> &Params {
        &self.params
    }

    /// Run one frame through the pipeline: downscale → MOG2 → morphological open → blobs.
    /// `bgr` is the hub's single decode: packed 8-bit BGR, `w*h*3` bytes.
    pub fn feed(&mut self, bgr: &[u8], w: u32, h: u32) -> Result<Verdict, String> {
        let p = &self.params;
        // Downscale to a fixed processing resolution (cheap + stable thresholds). Resampling is
        // channel-agnostic, so carrying BGR through an `RgbImage` resizes it correctly — the bytes
        // stay BGR, which is what the model wants.
        let src: RgbImage = ImageBuffer::<Rgb<u8>, _>::from_raw(w, h, bgr.to_vec())
            .ok_or_else(|| format!("motion: frame buffer does not match {w}x{h}x3"))?;
        let small = image::imageops::resize(
            &src,
            p.proc_w,
            p.proc_h,
            image::imageops::FilterType::Triangle, // = OpenCV's INTER_LINEAR
        );

        // Adaptive background subtraction → foreground mask (0/255).
        self.model.apply(small.as_raw(), self.mask.as_mut());
        // Morphological open: erode then dilate → drop speckle/sensor noise. `Norm::L1, k=1` is the
        // cross-shaped 3×3 kernel — which is exactly what OpenCV's `MORPH_ELLIPSE` at 3×3 is, so
        // this denoises identically to the pass this replaced.
        imageproc::morphology::open_mut(&mut self.mask, Norm::L1, 1);

        let score = self.mask.as_raw().iter().filter(|&&v| v > 0).count() as f64 / self.total * 100.0;

        // Connected components → keep blobs above min area; track count + largest bbox.
        let mut blobs = 0i64;
        let mut area = 0.0f64;
        let mut rect = Rect::default();
        for contour in imageproc::contours::find_contours::<i32>(&self.mask) {
            // Outer borders only — the equivalent of OpenCV's RETR_EXTERNAL.
            if contour.border_type != BorderType::Outer {
                continue;
            }
            let a = polygon_area(&contour.points);
            if a >= p.min_area {
                blobs += 1;
                if a > area {
                    area = a;
                    rect = bounding_rect(&contour.points);
                }
            }
        }

        Ok(Verdict {
            qualifies: score >= p.floor_pct && blobs > 0,
            score,
            area,
            blobs,
            rect,
        })
    }
}

/// The shoelace area of a closed polygon — what OpenCV's `contourArea` returns. **Not** the blob's
/// pixel count, which runs systematically larger (it includes the half-pixel border the polygon
/// cuts through). `MOTION_MIN_AREA` is calibrated against this, so the distinction is load-bearing:
/// counting pixels instead would quietly make every tenant's motion more sensitive.
fn polygon_area(pts: &[Point<i32>]) -> f64 {
    if pts.len() < 3 {
        return 0.0;
    }
    let mut acc = 0i64;
    for i in 0..pts.len() {
        let a = pts[i];
        let b = pts[(i + 1) % pts.len()];
        acc += i64::from(a.x) * i64::from(b.y) - i64::from(b.x) * i64::from(a.y);
    }
    (acc as f64 / 2.0).abs()
}

fn bounding_rect(pts: &[Point<i32>]) -> Rect {
    let (mut x0, mut y0) = (i32::MAX, i32::MAX);
    let (mut x1, mut y1) = (i32::MIN, i32::MIN);
    for p in pts {
        x0 = x0.min(p.x);
        y0 = y0.min(p.y);
        x1 = x1.max(p.x);
        y1 = y1.max(p.y);
    }
    if x0 > x1 {
        return Rect::default();
    }
    // OpenCV's boundingRect is inclusive of both endpoints → +1 on each axis.
    Rect { x: x0, y: y0, w: x1 - x0 + 1, h: y1 - y0 + 1 }
}

/// POST one normalized motion event to control (box-token). `jpeg` is the full-resolution frame
/// that triggered it, `(fw, fh)` its pixel size.
pub fn push_event(
    state: &Arc<AppState>,
    ip: &str,
    jpeg: &[u8],
    (fw, fh): (u32, u32),
    v: &Verdict,
    p: &Params,
) {
    // `v.rect` is in the downscaled processing space (proc_w×proc_h), but the frame we send is the
    // full-resolution original. Scale the box back to the frame's pixels so consumers (Draw
    // Detection Box, presence positioning) land it in the right place — otherwise it's shifted
    // top-left and too small.
    let sx = fw.max(1) as f32 / p.proc_w as f32;
    let sy = fh.max(1) as f32 / p.proc_h as f32;
    let ev = MotionEvent {
        camera_ip: ip.to_string(),
        score: (v.score * 100.0).round() / 100.0,
        area: v.area as i64,
        blobs: v.blobs,
        bbox: [
            (v.rect.x as f32 * sx).round() as i32,
            (v.rect.y as f32 * sy).round() as i32,
            (v.rect.w as f32 * sx).round() as i32,
            (v.rect.h as f32 * sy).round() as i32,
        ],
        at: app_core::timeutil::now_rfc3339(),
        frame_b64: Some(base64::engine::general_purpose::STANDARD.encode(jpeg)),
    };
    log::info!(
        "motion watcher[{ip}]: motion {:.2}% area={} blobs={}",
        v.score,
        v.area as i64,
        v.blobs
    );
    state.push_motion(&ev);
}
