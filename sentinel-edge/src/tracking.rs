//! On-edge object tracker (card: edge tracking) — one worker per camera running the YOLOX detector
//! ([`app_core::agent::detector`]) on a detection cadence, feeding the pure-Rust SORT/ByteTrack
//! [`app_core::tracking::MultiTracker`], and pushing `object_entered` / `object_exited` lifecycle
//! events to control (→ durable event_queue → `OnEvent`).
//!
//! Mirrors [`crate::motion`]'s reconciler + supervised-worker shape, but needs **no OpenCV**: the
//! detector + tracker work directly on the JPEG frames the edge already serves via go2rtc. Gated on
//! `TRACK_ENABLED=1` (off by default — YOLOX is heavier than MOG2, so tracking is opt-in until the
//! per-camera Track-node config lands). `TRACK_CLASS` (default `person`) and `TRACK_DETECT_MS`
//! (default 400) tune it.
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use app_core::agent::detector::{self, Detection};
use app_core::tracking::{MultiTracker, TrackEvent, TrackerConfig};

use crate::state::{AppState, TrackEventOut};

struct Cfg {
    class_name: String,
    class_id: u32,
    detect_gap: Duration,
}

impl Cfg {
    /// Returns `None` when tracking is disabled or the class name is unknown (→ manager no-ops).
    fn from_env() -> Option<Cfg> {
        let on = std::env::var("TRACK_ENABLED").map(|v| matches!(v.trim(), "1" | "true" | "yes")).unwrap_or(false);
        if !on {
            return None;
        }
        let class_name = std::env::var("TRACK_CLASS").ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).unwrap_or_else(|| "person".to_string());
        let class_id = detector::class_id_for_name(&class_name)?;
        let ms = std::env::var("TRACK_DETECT_MS").ok().and_then(|s| s.trim().parse().ok()).unwrap_or(400u64).max(100);
        Some(Cfg { class_name, class_id, detect_gap: Duration::from_millis(ms) })
    }
}

/// Spawn the tracking manager: every 30s reconcile one worker per camera that has a resolvable
/// stream, exactly like the motion manager. No-op (returns immediately) unless `TRACK_ENABLED=1`.
pub fn spawn(state: &Arc<AppState>) {
    let Some(cfg) = Cfg::from_env() else {
        log::info!("object tracking disabled (set TRACK_ENABLED=1 to enable)");
        return;
    };
    let cfg = Arc::new(cfg);
    let state = Arc::clone(state);
    std::thread::spawn(move || {
        log::info!("object tracker: manager started (class={}, detect every {}ms)", cfg.class_name, cfg.detect_gap.as_millis());
        let mut running: HashMap<String, Arc<AtomicBool>> = HashMap::new();
        loop {
            let desired: Vec<String> = {
                let devs = state.devices.lock().unwrap();
                devs.iter().filter(|d| has_stream(d)).map(|d| d.ip.clone()).collect()
            };
            let want: HashSet<&String> = desired.iter().collect();
            running.retain(|ip, stop| {
                let keep = want.contains(ip);
                if !keep {
                    stop.store(true, Ordering::Relaxed);
                }
                keep
            });
            for ip in &desired {
                if !running.contains_key(ip) {
                    let stop = Arc::new(AtomicBool::new(false));
                    running.insert(ip.clone(), Arc::clone(&stop));
                    let (st, ip2, c) = (Arc::clone(&state), ip.clone(), Arc::clone(&cfg));
                    std::thread::spawn(move || watch(st, ip2, c, stop));
                }
            }
            std::thread::sleep(Duration::from_secs(30));
        }
    });
}

fn has_stream(d: &app_core::devices::Device) -> bool {
    d.rtsp_url.as_deref().map(|u| !u.trim().is_empty()).unwrap_or(false)
}

/// One camera's supervised loop: run the tracker until stopped, retrying on error.
fn watch(state: Arc<AppState>, ip: String, cfg: Arc<Cfg>, stop: Arc<AtomicBool>) {
    log::info!("object tracker[{ip}]: starting");
    while !stop.load(Ordering::Relaxed) {
        run_track(&state, &ip, &cfg, &stop);
        if stop.load(Ordering::Relaxed) {
            break;
        }
        std::thread::sleep(Duration::from_secs(5));
    }
    log::info!("object tracker[{ip}]: stopped");
}

fn run_track(state: &Arc<AppState>, ip: &str, cfg: &Cfg, stop: &AtomicBool) {
    // Edge-tuned lifecycle: confirm fast (2 hits), exit after ~a few unmatched detection cycles.
    let tcfg = TrackerConfig { min_hits: 2, max_age: 8, ..TrackerConfig::default() };
    let mut tracker = MultiTracker::new(tcfg);
    let base = app_core::go2rtc::base();
    let agent = ureq::AgentBuilder::new().timeout(Duration::from_secs(3)).build();
    let debug = std::env::var("TRACK_DEBUG").map(|v| matches!(v.trim(), "1" | "true" | "yes")).unwrap_or(false);

    while !stop.load(Ordering::Relaxed) {
        let tick = std::time::Instant::now();
        // Latest decoded frame from local go2rtc (never the camera directly). Transient miss → retry.
        let jpeg = match app_core::go2rtc::frame_from(&agent, &base, ip) {
            Ok(b) => b,
            Err(_) => {
                std::thread::sleep(cfg.detect_gap);
                continue;
            }
        };
        // YOLOX on the JPEG (pure tract, no OpenCV), filtered to the tracked class.
        let dets: Vec<Detection> = match detector::detect(&jpeg) {
            Ok(v) => v.into_iter().filter(|d| d.class == cfg.class_id).collect(),
            Err(e) => {
                log::debug!("object tracker[{ip}]: detect failed: {e}");
                std::thread::sleep(cfg.detect_gap);
                continue;
            }
        };
        let now_ms = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
        let events = tracker.update(&dets, now_ms);
        let count = tracker.count() as i64;
        if debug {
            log::info!("object tracker[{ip}]: {} det, {} tracked, {} event(s)", dets.len(), count, events.len());
        }
        for ev in events {
            let out = match ev {
                TrackEvent::Entered { uuid, bbox, .. } => TrackEventOut {
                    kind: "object_entered".into(),
                    camera_ip: ip.to_string(),
                    uuid: uuid.to_string(),
                    class: cfg.class_name.clone(),
                    bbox: to_i32(bbox),
                    count,
                    dwell_secs: None,
                },
                TrackEvent::Exited { uuid, bbox, dwell_ms, .. } => TrackEventOut {
                    kind: "object_exited".into(),
                    camera_ip: ip.to_string(),
                    uuid: uuid.to_string(),
                    class: cfg.class_name.clone(),
                    bbox: to_i32(bbox),
                    count,
                    dwell_secs: Some((dwell_ms / 1000) as i64),
                },
            };
            log::info!("object tracker[{ip}]: {} {} (count={count})", out.kind, out.uuid);
            state.push_track_event(&out);
        }
        // Pace to the detection cadence (detection itself costs tens of ms).
        if let Some(rem) = cfg.detect_gap.checked_sub(tick.elapsed()) {
            std::thread::sleep(rem);
        }
    }
}

fn to_i32(b: [f32; 4]) -> [i32; 4] {
    [b[0] as i32, b[1] as i32, b[2] as i32, b[3] as i32]
}
