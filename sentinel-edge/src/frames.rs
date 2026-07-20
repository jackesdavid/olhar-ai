//! The per-camera **frame hub** — one fetch, one decode, shared by every consumer.
//!
//! Motion ([`crate::motion`]) and each object-tracking job ([`crate::tracking`]) all want the same
//! thing: the latest frame of a camera. They used to each poll go2rtc's `/api/frame.jpeg` and
//! decode the JPEG themselves, so a tracked camera decoded one frame up to **three** times per
//! cycle (motion's `imdecode`, the detector's internal decode, and `detector::dimensions`'s second
//! decode when the job had lines or a zone). On the little x86 appliance we ship, that was the
//! difference between a box that idles cool and one that doesn't.
//!
//! Now: **one source thread per camera** pulls the frame, decodes it once, runs that camera's MOG2
//! pass, and publishes the result. Consumers subscribe and read. A camera with four tracking jobs
//! costs exactly one decode, same as a camera with none.
//!
//! The published [`Frame`] deliberately carries **plain byte buffers**, not an `opencv::Mat`: a
//! `Mat` is not `Sync`, so it cannot be shared behind an `Arc` across the worker threads. A
//! consumer that needs OpenCV can wrap `bgr` back into a borrowed `Mat` for free
//! (`Mat::new_rows_cols_with_data`) — no copy, no second decode.
//!
//! Publishing is a **latest-value slot**, not a queue: a slow consumer misses frames rather than
//! backing the source up, which is the right trade for live video (a stale frame is worthless).

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::motion::{MotionDetector, Params};
use crate::state::AppState;

/// One camera frame, decoded once and shared by every consumer of that camera.
///
/// Carries the decoded pixels, not the JPEG: the only consumer that still wants the compressed
/// bytes is the motion event's `frame_b64`, and the source thread already has them in hand when it
/// builds that event. (A future consumer that needs the JPEG can carry an `Arc<Vec<u8>>` here for
/// the price of one refcount bump — the source holds it anyway.)
pub struct Frame {
    /// Monotonic per-camera counter. A consumer sampling below the hub's rate uses this to skip
    /// a frame it has already processed.
    pub seq: u64,
    /// The decoded frame: packed 8-bit BGR, `w * h * 3` bytes, row-major.
    pub bgr: Arc<Vec<u8>>,
    pub w: u32,
    pub h: u32,
    /// When this camera last had a frame whose **raw** MOG2 verdict qualified — not the 4s-throttled
    /// emit to control. Carried on *every* frame (not just the ones with motion) so a consumer
    /// sampling slower than the hub — the tracker, at `detect_ms` — cannot miss a burst of motion
    /// that happened between the two frames it actually looked at.
    pub motion_at: Option<Instant>,
}

/// A camera's latest-value slot. `Condvar` so a consumer can block until the next frame instead of
/// spinning on the mutex.
#[derive(Default)]
struct Slot {
    latest: Mutex<Option<Arc<Frame>>>,
    ready: Condvar,
}

fn slots() -> &'static Mutex<HashMap<String, Arc<Slot>>> {
    static SLOTS: OnceLock<Mutex<HashMap<String, Arc<Slot>>>> = OnceLock::new();
    SLOTS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Get-or-create a camera's slot. Created on demand from **either** side, because a tracking
/// worker can subscribe before that camera's source thread has started (or while it is retrying a
/// dead stream) — it just blocks on an empty slot until frames arrive.
fn slot_for(ip: &str) -> Arc<Slot> {
    let mut map = slots().lock().unwrap();
    Arc::clone(map.entry(ip.to_string()).or_default())
}

/// A consumer's handle on one camera's frames.
pub struct Sub {
    slot: Arc<Slot>,
    last_seq: u64,
}

/// Subscribe to a camera's frames. Cheap; a consumer holds one of these for its lifetime.
pub fn subscribe(ip: &str) -> Sub {
    Sub { slot: slot_for(ip), last_seq: 0 }
}

impl Sub {
    /// Block until a frame **newer than the last one this subscriber saw** is published, or
    /// `timeout` elapses. `None` on timeout — the camera is down, warming up, or its source thread
    /// is between retries; the caller should loop (and re-check its stop flag).
    pub fn next(&mut self, timeout: Duration) -> Option<Arc<Frame>> {
        let latest = self.slot.latest.lock().unwrap();
        let fresh = |f: &Option<Arc<Frame>>| f.as_ref().map(|f| f.seq > self.last_seq).unwrap_or(false);
        let (latest, _) = self
            .slot
            .ready
            .wait_timeout_while(latest, timeout, |f| !fresh(f))
            .unwrap();
        let frame = Arc::clone(latest.as_ref()?);
        if frame.seq <= self.last_seq {
            return None; // timed out with nothing new
        }
        self.last_seq = frame.seq;
        Some(frame)
    }
}

/// Spawn the hub manager: every 30s reconcile one source thread per camera that has a resolvable
/// stream URL, starting new ones and stopping sources whose camera vanished. (Same reconciler shape
/// the motion watcher had — this replaces `motion::spawn`.)
pub fn spawn(state: &Arc<AppState>) {
    let state = Arc::clone(state);
    std::thread::spawn(move || {
        log::info!("frame hub: manager started (one decode per camera; pure-Rust MOG2 motion)");
        let mut running: HashMap<String, Arc<AtomicBool>> = HashMap::new();
        loop {
            let desired: Vec<String> = {
                let devs = state.devices.lock().unwrap();
                devs.iter().filter(|d| has_stream(d)).map(|d| d.ip.clone()).collect()
            };
            let want: HashSet<&String> = desired.iter().collect();
            // Stop sources for cameras that disappeared.
            running.retain(|ip, stop| {
                let keep = want.contains(ip);
                if !keep {
                    stop.store(true, Ordering::Relaxed);
                }
                keep
            });
            // Start sources for new cameras.
            for ip in &desired {
                if !running.contains_key(ip) {
                    let stop = Arc::new(AtomicBool::new(false));
                    running.insert(ip.clone(), Arc::clone(&stop));
                    let (st, ip2) = (Arc::clone(&state), ip.clone());
                    std::thread::spawn(move || source(st, ip2, stop));
                }
            }
            std::thread::sleep(Duration::from_secs(30));
        }
    });
}

/// True when go2rtc is serving this camera (its RTSP resolved) — the hub reads frames from the
/// LOCAL go2rtc, never the camera directly, so a resolved stream is the gate.
fn has_stream(d: &app_core::devices::Device) -> bool {
    d.rtsp_url.as_deref().map(|u| !u.trim().is_empty()).unwrap_or(false)
}

/// One camera's supervised source loop: run the pipeline until stopped, retrying on error.
fn source(state: Arc<AppState>, ip: String, stop: Arc<AtomicBool>) {
    let params = motion_params(&state);
    log::info!(
        "frame hub[{ip}]: starting (fps={}, floor={}%, min_interval={}s, mog2_var={})",
        params.fps,
        params.floor_pct,
        params.min_interval.as_secs(),
        params.var_threshold
    );
    while !stop.load(Ordering::Relaxed) {
        if let Err(e) = run(&state, &ip, &stop) {
            log::warn!("frame hub[{ip}]: {e}; retrying in 5s");
        }
        if stop.load(Ordering::Relaxed) {
            break;
        }
        std::thread::sleep(Duration::from_secs(5));
    }
    log::info!("frame hub[{ip}]: stopped");
}

/// Env tunables, with the box's `recording_config` allowed to override the MOG2 sensitivity.
fn motion_params(state: &Arc<AppState>) -> Params {
    let mut params = Params::from_env();
    let cfg_var = state.recording.config.motion_var_threshold;
    if cfg_var > 0.0 {
        params.var_threshold = cfg_var;
    }
    params
}

/// Fetch → decode once → MOG2 → publish, until the stop flag is set (or the pipeline errors → the
/// supervisor retries with a fresh background model).
fn run(state: &Arc<AppState>, ip: &str, stop: &AtomicBool) -> Result<(), String> {
    let params = motion_params(state);
    let frame_gap = params.frame_gap();
    let mut detector = MotionDetector::new(params);

    // Pull the latest decoded frame from the local go2rtc (`/api/frame.jpeg`) — always fresh, with
    // NO RTSP client buffer that can fall behind live motion (that buffer was why `VideoCapture`
    // lagged). Reuses go2rtc's authenticated decode; never touches the camera.
    let base = app_core::go2rtc::base();
    let agent = ureq::AgentBuilder::new().timeout(Duration::from_secs(3)).build();
    let slot = slot_for(ip);

    // `MOTION_DEBUG=1` logs the live peak score + measured fps every ~2s (even below the floor /
    // throttle) so detection can be verified in real time against movement.
    let debug = std::env::var("MOTION_DEBUG").map(|v| matches!(v.trim(), "1" | "true" | "yes")).unwrap_or(false);
    let mut dbg_at = Instant::now();
    let mut dbg_frames = 0u32;
    let mut dbg_max = 0.0f64;

    let mut seq = 0u64;
    let mut motion_at: Option<Instant> = None;
    let mut last_emit: Option<Instant> = None;

    // #120 HD face capture: grab the main/HD stream instead of the preview substream when the box
    // config asks for it. The SAME frame feeds motion + face detection, so there's no temporal
    // mismatch and no bbox scaling — faces are just bigger.
    let hd = state.recording.config.face_capture_stream.eq_ignore_ascii_case("hd");
    let grab: fn(&ureq::Agent, &str, &str) -> Result<Vec<u8>, String> =
        if hd { app_core::go2rtc::frame_from_hd } else { app_core::go2rtc::frame_from };
    log::info!("frame hub[{ip}]: face capture = {} stream", if hd { "HD (main)" } else { "preview (sub)" });
    // Resource measurement: avg grab+decode cost + fps per mode, logged every ~10s → the Logs page,
    // so substream vs HD (±GPU decode) can be compared without touching the box.
    let mut cap_at = Instant::now();
    let mut cap_us = 0u128;
    let mut cap_n = 0u32;

    while !stop.load(Ordering::Relaxed) {
        let t0 = Instant::now();
        // Transient errors (stream warming, a dropped frame) skip this tick — keep the background
        // model and try again, never tear the source down.
        let jpeg = match grab(&agent, &base, ip) {
            Ok(b) => b,
            Err(_) => {
                std::thread::sleep(frame_gap);
                continue;
            }
        };
        // The one decode. Every consumer of this camera works off the buffer it produces.
        let Some((bgr, w, h)) = decode_bgr(&jpeg) else {
            std::thread::sleep(frame_gap);
            continue;
        };
        // Measurement: grab+decode is the capture cost we're comparing across modes.
        cap_us += t0.elapsed().as_micros();
        cap_n += 1;
        if cap_at.elapsed() >= Duration::from_secs(10) {
            log::info!(
                "capture[{ip}]: mode={} {}x{} avg_decode={:.1}ms {} frames ({:.1} fps)",
                if hd { "HD" } else { "preview" },
                w, h,
                cap_us as f64 / cap_n.max(1) as f64 / 1000.0,
                cap_n,
                cap_n as f64 / cap_at.elapsed().as_secs_f64(),
            );
            cap_at = Instant::now();
            cap_us = 0;
            cap_n = 0;
        }

        let verdict = detector.feed(&bgr, w, h)?;
        if verdict.qualifies {
            motion_at = Some(t0);
        }

        if debug {
            dbg_frames += 1;
            dbg_max = dbg_max.max(verdict.score);
            if dbg_at.elapsed() >= Duration::from_secs(2) {
                let fps = f64::from(dbg_frames) / dbg_at.elapsed().as_secs_f64();
                log::info!("frame hub[{ip}]: live peak {dbg_max:.2}% over 2s @ {fps:.1}fps");
                dbg_at = Instant::now();
                dbg_frames = 0;
                dbg_max = 0.0;
            }
        }

        // Publish before emitting: the tracker's gate should see this frame's motion as early as
        // possible, and the POST to control below can block for as long as its timeout allows.
        seq += 1;
        // Keep the decoded BGR reachable after publishing (Arc, cheap clone) so face detection can
        // reuse it — the hub decodes once; nothing decodes the JPEG a second time.
        let bgr = Arc::new(bgr);
        publish(&slot, Frame { seq, bgr: Arc::clone(&bgr), w, h, motion_at });

        // The emit — and ONLY the emit — is throttled. The per-frame verdict above is not.
        let throttled = last_emit.map(|t| t.elapsed() < detector.params().min_interval).unwrap_or(false);
        if verdict.qualifies && !throttled {
            last_emit = Some(Instant::now());
            crate::motion::push_event(state, ip, &jpeg, (w, h), &verdict, detector.params());
            // Same gate + throttle as motion: on a qualifying frame, detect faces on the hub's BGR
            // and emit one `face_detected` per face → the cloud recognition pipeline (#117).
            crate::face::detect_and_push(state, ip, &bgr, (w, h), &jpeg);
        }

        // Cap the processing rate so a source uses a bounded slice of CPU.
        if let Some(rem) = frame_gap.checked_sub(t0.elapsed()) {
            std::thread::sleep(rem);
        }
    }
    Ok(())
}

fn publish(slot: &Arc<Slot>, frame: Frame) {
    *slot.latest.lock().unwrap() = Some(Arc::new(frame));
    slot.ready.notify_all();
}

/// Decode a JPEG to packed 8-bit **BGR** — the layout both the motion model and the YOLOX detector
/// want (`detector::detect_bgr`). `None` on a malformed/empty frame, which the caller treats as a
/// dropped tick rather than an error.
///
/// The `image` crate has no BGR pixel type, so we decode to RGB and swap in place. That swap is the
/// price of dropping OpenCV's `imdecode`, and it is cheap next to the decode itself.
fn decode_bgr(jpeg: &[u8]) -> Option<(Vec<u8>, u32, u32)> {
    let img = image::load_from_memory(jpeg).ok()?.to_rgb8();
    let (w, h) = (img.width(), img.height());
    if w == 0 || h == 0 {
        return None;
    }
    let mut buf = img.into_raw();
    for px in buf.chunks_exact_mut(3) {
        px.swap(0, 2);
    }
    Some((buf, w, h))
}
