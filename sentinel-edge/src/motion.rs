//! Motion detection on the edge (OpenCV MOG2 adaptive background subtraction) — the *algorithm*,
//! not the threading. One [`MotionDetector`] per camera holds that camera's background model and
//! is fed one decoded frame at a time by the frame hub ([`crate::frames`]), which owns the
//! per-camera thread and the single JPEG decode every consumer shares.
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
//! Requires libopencv on the host (see `install.sh`). Tunables (env): `MOTION_FPS` (4),
//! `MOTION_FLOOR_PCT` (0.5), `MOTION_MIN_INTERVAL_SECS` (4), `MOTION_MIN_AREA` (300),
//! `MOTION_PROC_W`/`MOTION_PROC_H` (480×270), `MOTION_MOG2_HISTORY` (333), `MOTION_MOG2_VAR` (16).

use std::sync::Arc;
use std::time::Duration;

use base64::Engine as _;
use opencv::{core, imgproc, video};

use crate::state::{AppState, MotionEvent};

/// The MOG2 history is counted in **frames**, not seconds — so it has to be derived from the
/// frame rate or the background model's time constant silently changes with it. These two
/// defaults are the same ~83s of adaptation the watcher has always had (it was 500 frames at
/// 6 fps); dropping `MOTION_FPS` to 4 without rescaling history would have stretched it to ~125s.
const DEFAULT_FPS: f64 = 4.0;
const DEFAULT_HISTORY: f64 = 333.0; // ≈ DEFAULT_FPS × 83s

pub struct Params {
    pub fps: u32,
    pub floor_pct: f64,
    pub min_interval: Duration,
    pub min_area: f64,
    pub proc_w: i32,
    pub proc_h: i32,
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
            proc_w: num("MOTION_PROC_W", 480.0) as i32,
            proc_h: num("MOTION_PROC_H", 270.0) as i32,
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
    pub rect: core::Rect,
}

/// One camera's MOG2 background model + the scratch buffers its pipeline reuses. Fed one decoded
/// BGR frame per tick by the hub; never touches the network or the camera.
pub struct MotionDetector {
    params: Params,
    bg: core::Ptr<video::BackgroundSubtractorMOG2>,
    kernel: core::Mat,
    small: core::Mat,
    mask: core::Mat,
    opened: core::Mat,
    total: f64,
}

impl MotionDetector {
    pub fn new(params: Params) -> opencv::Result<Self> {
        // history, varThreshold, detectShadows=false (we don't want shadow pixels as motion).
        let bg = video::create_background_subtractor_mog2(params.history, params.var_threshold, false)?;
        let kernel = imgproc::get_structuring_element(
            imgproc::MORPH_ELLIPSE,
            core::Size::new(3, 3),
            core::Point::new(-1, -1),
        )?;
        let total = (params.proc_w * params.proc_h).max(1) as f64;
        Ok(MotionDetector {
            params,
            bg,
            kernel,
            small: core::Mat::default(),
            mask: core::Mat::default(),
            opened: core::Mat::default(),
            total,
        })
    }

    pub fn params(&self) -> &Params {
        &self.params
    }

    /// Run one frame through the pipeline: downscale → MOG2 → morphological open → contours.
    /// `frame` is a full-resolution BGR image (the hub's single decode).
    pub fn feed(&mut self, frame: &core::Mat) -> opencv::Result<Verdict> {
        let p = &self.params;
        // Downscale to a fixed processing resolution (cheap + stable thresholds).
        imgproc::resize(
            frame,
            &mut self.small,
            core::Size::new(p.proc_w, p.proc_h),
            0.0,
            0.0,
            imgproc::INTER_LINEAR,
        )?;
        // Adaptive background subtraction → foreground mask (0/255).
        opencv::prelude::BackgroundSubtractorTrait::apply(&mut self.bg, &self.small, &mut self.mask, -1.0)?;
        // Morphological open: erode then dilate → drop speckle/sensor noise.
        imgproc::morphology_ex(
            &self.mask,
            &mut self.opened,
            imgproc::MORPH_OPEN,
            &self.kernel,
            core::Point::new(-1, -1),
            1,
            core::BORDER_CONSTANT,
            imgproc::morphology_default_border_value()?,
        )?;

        let score = core::count_non_zero(&self.opened)? as f64 / self.total * 100.0;

        // Connected components → keep blobs above min area; track count + largest bbox.
        let mut contours: core::Vector<core::Vector<core::Point>> = core::Vector::new();
        imgproc::find_contours(
            &self.opened,
            &mut contours,
            imgproc::RETR_EXTERNAL,
            imgproc::CHAIN_APPROX_SIMPLE,
            core::Point::new(0, 0),
        )?;
        let mut blobs = 0i64;
        let mut area = 0.0f64;
        let mut rect = core::Rect::new(0, 0, 0, 0);
        for c in contours.iter() {
            let a = imgproc::contour_area(&c, false)?;
            if a >= p.min_area {
                blobs += 1;
                if a > area {
                    area = a;
                    rect = imgproc::bounding_rect(&c)?;
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

/// POST one normalized motion event to control (box-token). `jpeg` is the full-resolution frame
/// that triggered it, `(fw, fh)` its pixel size.
pub fn push_event(
    state: &Arc<AppState>,
    ip: &str,
    jpeg: &[u8],
    (fw, fh): (i32, i32),
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
            (v.rect.width as f32 * sx).round() as i32,
            (v.rect.height as f32 * sy).round() as i32,
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
