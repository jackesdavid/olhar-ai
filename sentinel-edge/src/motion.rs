//! Continuous motion detection on the edge (OpenCV MOG2 adaptive background subtraction).
//!
//! A core edge capability: the edge is the only thing near the cameras, so it owns
//! continuous motion. One background thread per camera reads that camera's stream locally
//! (no tailnet cost), runs MOG2 → denoise → contours every frame, and POSTs ONE normalized
//! motion event to control when motion clears a small floor (+ a min-interval throttle). It
//! is graph-agnostic — per-graph sensitivity is applied in the cloud (control → Redis →
//! `OnEvent("motion")`). A manager thread reconciles the per-camera watcher set against the
//! live device list.
//!
//! Requires libopencv on the host (see `install.sh`). Tunables (env): `MOTION_FPS` (6),
//! `MOTION_FLOOR_PCT` (0.5), `MOTION_MIN_INTERVAL_SECS` (10), `MOTION_MIN_AREA` (300),
//! `MOTION_PROC_W`/`MOTION_PROC_H` (480×270), `MOTION_MOG2_HISTORY` (500), `MOTION_MOG2_VAR` (16).

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use opencv::{core, imgproc, prelude::*, video, videoio};

use crate::state::{AppState, MotionEvent};

struct Params {
    fps: u32,
    floor_pct: f64,
    min_interval: Duration,
    min_area: f64,
    proc_w: i32,
    proc_h: i32,
    history: i32,
    var_threshold: f64,
}

impl Params {
    fn from_env() -> Self {
        let num = |k: &str, d: f64| -> f64 {
            std::env::var(k)
                .ok()
                .and_then(|v| v.trim().parse().ok())
                .unwrap_or(d)
        };
        Params {
            fps: num("MOTION_FPS", 6.0).max(1.0) as u32,
            floor_pct: num("MOTION_FLOOR_PCT", 0.5),
            min_interval: Duration::from_secs(num("MOTION_MIN_INTERVAL_SECS", 10.0) as u64),
            min_area: num("MOTION_MIN_AREA", 300.0),
            proc_w: num("MOTION_PROC_W", 480.0) as i32,
            proc_h: num("MOTION_PROC_H", 270.0) as i32,
            history: num("MOTION_MOG2_HISTORY", 500.0) as i32,
            var_threshold: num("MOTION_MOG2_VAR", 16.0),
        }
    }
}

/// Spawn the manager thread: every 30s reconcile one watcher per camera that has a
/// resolvable stream URL, starting new ones and stopping watchers whose camera vanished.
pub fn spawn(state: &Arc<AppState>) {
    let state = Arc::clone(state);
    std::thread::spawn(move || {
        log::info!("motion watcher: manager started (OpenCV MOG2)");
        let mut running: HashMap<String, Arc<AtomicBool>> = HashMap::new();
        loop {
            let desired: Vec<(String, String)> = {
                let devs = state.devices.lock().unwrap();
                devs.iter()
                    .filter_map(|d| stream_url(d).map(|u| (d.ip.clone(), u)))
                    .collect()
            };
            let want: HashSet<&String> = desired.iter().map(|(ip, _)| ip).collect();
            // Stop watchers for cameras that disappeared.
            running.retain(|ip, stop| {
                let keep = want.contains(ip);
                if !keep {
                    stop.store(true, Ordering::Relaxed);
                }
                keep
            });
            // Start watchers for new cameras.
            for (ip, url) in &desired {
                if !running.contains_key(ip) {
                    let stop = Arc::new(AtomicBool::new(false));
                    running.insert(ip.clone(), Arc::clone(&stop));
                    let (st, ip2, url2) = (Arc::clone(&state), ip.clone(), url.clone());
                    std::thread::spawn(move || watch(st, ip2, url2, stop));
                }
            }
            std::thread::sleep(Duration::from_secs(30));
        }
    });
}

/// The local go2rtc RTSP re-stream for a camera go2rtc is serving — `cam_<ip>` is the preview
/// (substream) feed, ALREADY authenticated to the camera by go2rtc, so we open it with no
/// creds and never touch the camera directly. Returns `None` for cameras without a resolved
/// stream. Base overridable via `MOTION_GO2RTC_RTSP` (default `rtsp://127.0.0.1:8554`).
fn stream_url(d: &app_core::devices::Device) -> Option<String> {
    let has_stream = d.rtsp_url.as_deref().map(|u| !u.trim().is_empty()).unwrap_or(false);
    if !has_stream {
        return None;
    }
    let base = std::env::var("MOTION_GO2RTC_RTSP")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "rtsp://127.0.0.1:8554".to_string());
    Some(format!("{}/{}", base.trim_end_matches('/'), app_core::go2rtc::stream_name(&d.ip)))
}

/// One camera's supervised loop: (re)connect + run the capture until stopped.
fn watch(state: Arc<AppState>, ip: String, url: String, stop: Arc<AtomicBool>) {
    let p = Params::from_env();
    log::info!(
        "motion watcher[{ip}]: starting (fps={}, floor={}%, min_interval={}s)",
        p.fps,
        p.floor_pct,
        p.min_interval.as_secs()
    );
    while !stop.load(Ordering::Relaxed) {
        if let Err(e) = run_capture(&state, &ip, &url, &p, &stop) {
            log::warn!("motion watcher[{ip}]: {e}; reconnecting in 5s");
        }
        if stop.load(Ordering::Relaxed) {
            break;
        }
        std::thread::sleep(Duration::from_secs(5));
    }
    log::info!("motion watcher[{ip}]: stopped");
}

fn err(msg: impl Into<String>) -> opencv::Error {
    opencv::Error::new(core::StsError, msg.into())
}

/// Open the stream and run the MOG2 motion pipeline frame-by-frame until the stream ends
/// (returns an error to trigger a reconnect) or the stop flag is set.
fn run_capture(
    state: &Arc<AppState>,
    ip: &str,
    url: &str,
    p: &Params,
    stop: &AtomicBool,
) -> opencv::Result<()> {
    let mut cap = videoio::VideoCapture::from_file(url, videoio::CAP_FFMPEG)?;
    if !cap.is_opened()? {
        return Err(err(format!("cannot open stream {url}")));
    }
    // history, varThreshold, detectShadows=false (we don't want shadow pixels as motion).
    let mut bg = video::create_background_subtractor_mog2(p.history, p.var_threshold, false)?;
    let kernel = imgproc::get_structuring_element(
        imgproc::MORPH_ELLIPSE,
        core::Size::new(3, 3),
        core::Point::new(-1, -1),
    )?;
    let mut frame = core::Mat::default();
    let mut small = core::Mat::default();
    let mut mask = core::Mat::default();
    let mut opened = core::Mat::default();
    let total = (p.proc_w * p.proc_h).max(1) as f64;
    let frame_gap = Duration::from_millis(1000 / u64::from(p.fps));
    let mut last_emit: Option<Instant> = None;

    while !stop.load(Ordering::Relaxed) {
        let t0 = Instant::now();
        if !cap.read(&mut frame)? || frame.empty() {
            return Err(err("stream read failed / ended"));
        }
        // Downscale to a fixed processing resolution (cheap + stable thresholds).
        imgproc::resize(
            &frame,
            &mut small,
            core::Size::new(p.proc_w, p.proc_h),
            0.0,
            0.0,
            imgproc::INTER_LINEAR,
        )?;
        // Adaptive background subtraction → foreground mask (0/255).
        opencv::prelude::BackgroundSubtractorTrait::apply(&mut bg, &small, &mut mask, -1.0)?;
        // Morphological open: erode then dilate → drop speckle/sensor noise.
        imgproc::morphology_ex(
            &mask,
            &mut opened,
            imgproc::MORPH_OPEN,
            &kernel,
            core::Point::new(-1, -1),
            1,
            core::BORDER_CONSTANT,
            imgproc::morphology_default_border_value()?,
        )?;

        let pct = core::count_non_zero(&opened)? as f64 / total * 100.0;

        // Connected components → keep blobs above min area; track count + largest bbox.
        let mut contours: core::Vector<core::Vector<core::Point>> = core::Vector::new();
        imgproc::find_contours(
            &opened,
            &mut contours,
            imgproc::RETR_EXTERNAL,
            imgproc::CHAIN_APPROX_SIMPLE,
            core::Point::new(0, 0),
        )?;
        let mut blobs = 0i64;
        let mut largest_area = 0.0f64;
        let mut largest_rect = core::Rect::new(0, 0, 0, 0);
        for c in contours.iter() {
            let area = imgproc::contour_area(&c, false)?;
            if area >= p.min_area {
                blobs += 1;
                if area > largest_area {
                    largest_area = area;
                    largest_rect = imgproc::bounding_rect(&c)?;
                }
            }
        }

        let qualifies = pct >= p.floor_pct && blobs > 0;
        let throttle_ok = last_emit
            .map(|t| t.elapsed() >= p.min_interval)
            .unwrap_or(true);
        if qualifies && throttle_ok {
            last_emit = Some(Instant::now());
            let ev = MotionEvent {
                camera_ip: ip.to_string(),
                score: (pct * 100.0).round() / 100.0,
                area: largest_area as i64,
                blobs,
                bbox: [
                    largest_rect.x,
                    largest_rect.y,
                    largest_rect.width,
                    largest_rect.height,
                ],
                at: app_core::alerts::now_rfc3339(),
            };
            log::info!(
                "motion watcher[{ip}]: motion {pct:.2}% area={} blobs={blobs}",
                largest_area as i64
            );
            state.push_motion(&ev);
        }

        // Cap the processing rate so a watcher uses a bounded slice of CPU.
        if let Some(rem) = frame_gap.checked_sub(t0.elapsed()) {
            std::thread::sleep(rem);
        }
    }
    Ok(())
}
