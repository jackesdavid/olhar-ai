//! On-edge object tracker (card: edge tracking) — one worker per **tracking job** (a `camera` +
//! `class` pair, e.g. "person on the front door cam") running the YOLOX detector
//! ([`app_core::agent::detector`]) on a detection cadence, feeding the pure-Rust SORT/ByteTrack
//! [`app_core::tracking::MultiTracker`], and pushing `object_entered` / `object_exited` /
//! `line_crossed` lifecycle events to control (→ durable event_queue → `OnEvent`).
//!
//! Mirrors [`crate::motion`]'s reconciler + supervised-worker shape, but needs **no OpenCV**: the
//! detector + tracker work directly on the JPEG frames the edge already serves via go2rtc.
//!
//! **Jobs, not cameras, are the unit of work**: the same camera can be tracked for several
//! classes at once (person + car), each from its own `OnEvent` node in the graph editor (camera +
//! class + — for `line_crossed` — a line, all drawn in that node's property panel) — each such
//! node is one independent job with its own uuid space and its own `MultiTracker`, so tracking two
//! classes on one camera never conflates or duplicates entities.
//!
//! Config resolves from control's per-tenant `tracking_config` (`GET /config/tracking`,
//! box-authed, RE-FETCHED every ~30s by [`spawn`]'s reconcile loop — enabling a graph or editing
//! an `OnEvent` node's camera/class/tuning takes effect live, no edge restart needed):
//! `detect_ms` is box-global; `trackers` is the job list (each `{camera, class, lines[]}`),
//! extracted from every enabled graph's matching `OnEvent` nodes on save — **a job's mere
//! presence in `trackers[]` is what enables tracking for it**, per-node, not some separate
//! box-wide switch: wiring an `OnEvent(object_entered/object_exited/line_crossed)` node to a
//! camera in a graph is both the config AND the enable action. When control is unreachable or
//! has no jobs configured yet, `TRACK_ENABLED=1` + `TRACK_CLASS` (default
//! `person`) + `TRACK_CAMERA` (default `*` — every camera with a stream, like the pre-per-job
//! behaviour; a specific id/ip/name/`"first"` scopes to one) + `TRACK_LINES` (optional,
//! `name:x1,y1,x2,y2;...` normalized `0.0..1.0` frame coords) synthesize a single dev/single-box
//! fallback job.
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use app_core::agent::detector::{self, Detection};
use app_core::tracking::{Line, MultiTracker, Polygon, TrackEvent, TrackerConfig, ZoneTrigger};

use crate::state::{AppState, TrackEventOut};

/// Tracker-tuning defaults — edge-calibrated (not `TrackerConfig::default()`'s generic
/// ByteTrack-ish values): confirm fast, tolerate ~10s of unmatched detection cycles at the
/// default 400ms cadence before exiting. A node's OnEvent property panel shows these same
/// numbers as its own displayed defaults (`sentinel-app`'s `effectiveKind`/`chooseEvent`) — keep
/// the two in sync. `max_age: 8` (~3.2s) was too tight in practice: a live test showed a
/// continuously-present, stationary person getting two Enter/Exit pairs from ordinary detection
/// jitter alone.
fn default_min_hits() -> u32 {
    2
}
fn default_max_age() -> u32 {
    25
}
fn default_iou_thresh() -> f32 {
    0.3
}
fn default_high_thresh() -> f32 {
    0.5
}
fn default_low_thresh() -> f32 {
    0.1
}

/// One resolved tracking job: a class to detect on a camera (`"*"` = every camera with a stream,
/// matching the pre-per-job behaviour; a specific id/ip/name/`"first"` scopes to one), that job's
/// own crossing lines, an optional zone (when set, `object_entered`/`object_exited` mean
/// entering/leaving THIS polygon specifically — see `run_track`'s event translation — instead of
/// the whole-frame track lifecycle), this job's own `TrackerConfig` tuning (per-node, so two jobs
/// on the same box can be tuned independently), and whether to log verbose algorithm internals
/// (state transitions, per-cycle counts) for this job — mirrors the originating graph's own
/// "Debug" toggle (`Graph.debug`), OR'd with the box-wide `TRACK_DEBUG` env var at log time. The
/// manager reconciles one worker per (job, resolved camera ip).
#[derive(Debug, Clone)]
struct Job {
    camera: String,
    class_name: String,
    class_id: u32,
    lines: Vec<Line>,
    zone: Option<Polygon>,
    zone_trigger: ZoneTrigger,
    min_hits: u32,
    max_age: u32,
    iou_thresh: f32,
    high_thresh: f32,
    low_thresh: f32,
    debug: bool,
}

impl Job {
    fn tracker_config(&self) -> TrackerConfig {
        TrackerConfig {
            high_thresh: self.high_thresh,
            low_thresh: self.low_thresh,
            iou_thresh: self.iou_thresh,
            min_hits: self.min_hits,
            max_age: self.max_age,
        }
    }
    /// A fingerprint of this job's full config, folded into `spawn`'s reconcile key alongside
    /// the resolved camera ip — so an in-place edit (same camera+class, different tuning/zone/
    /// class/etc., saved from the graph editor) produces a NEW key, causing the old worker to
    /// stop and a fresh one to start with the new config, instead of the reconcile loop seeing
    /// "same job, nothing to do" and leaving a stale worker running until the box restarts.
    fn fingerprint(&self) -> String {
        format!("{self:?}")
    }
}

struct Cfg {
    detect_gap: Duration,
    jobs: Vec<Job>,
}

impl Cfg {
    /// Resolve tracking config for this box: control's per-tenant `tracking_config` (`GET
    /// /config/tracking`, box-authed) is authoritative when it has any jobs — a job existing in
    /// `trackers[]` IS the enable signal, there is no separate box-level on/off switch. Otherwise
    /// (control unreachable, or a box not yet configured from the graph editor) `TRACK_ENABLED=1`
    /// synthesizes one dev/single-box fallback job from the `TRACK_*` env vars. Returns `None`
    /// when there are no configured jobs and no `TRACK_ENABLED=1` fallback, or every configured
    /// job's class is unrecognized.
    fn load(control_url: &str) -> Option<Cfg> {
        let remote = fetch_remote(control_url);
        let env_on = std::env::var("TRACK_ENABLED").map(|v| matches!(v.trim(), "1" | "true" | "yes")).unwrap_or(false);
        let ms = remote
            .as_ref()
            .and_then(|r| r.detect_ms)
            .or_else(|| std::env::var("TRACK_DETECT_MS").ok().and_then(|s| s.trim().parse().ok()))
            .unwrap_or(400u64)
            .max(100);

        let mut jobs: Vec<Job> = remote
            .map(|r| r.trackers)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|t| {
                let class_id = detector::class_id_for_name(&t.class)?;
                Some(Job {
                    camera: t.camera,
                    class_name: t.class,
                    class_id,
                    lines: t.lines,
                    zone: t.zone,
                    zone_trigger: t.zone_trigger,
                    min_hits: t.min_hits,
                    max_age: t.max_age,
                    iou_thresh: t.iou_thresh,
                    high_thresh: t.high_thresh,
                    low_thresh: t.low_thresh,
                    debug: t.debug,
                })
            })
            .collect();

        // Dev/single-box fallback: only when explicitly requested via TRACK_ENABLED=1 (control
        // has no jobs for this box — unreachable, or none configured yet from the graph editor)
        // — synthesize one from the env vars, same shape tracking had before per-job config
        // landed. No env-var zone syntax — a fallback job is always whole-frame; draw a zone from
        // the graph editor's OnEvent property panel instead.
        if jobs.is_empty() && env_on {
            let class_name = std::env::var("TRACK_CLASS").ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).unwrap_or_else(|| "person".to_string());
            if let Some(class_id) = detector::class_id_for_name(&class_name) {
                let camera = std::env::var("TRACK_CAMERA").ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).unwrap_or_else(|| "*".to_string());
                let lines = std::env::var("TRACK_LINES").ok().map(|s| parse_lines(&s)).unwrap_or_default();
                jobs.push(Job {
                    camera,
                    class_name,
                    class_id,
                    lines,
                    zone: None,
                    zone_trigger: ZoneTrigger::Intersects,
                    min_hits: default_min_hits(),
                    max_age: default_max_age(),
                    iou_thresh: default_iou_thresh(),
                    high_thresh: default_high_thresh(),
                    low_thresh: default_low_thresh(),
                    debug: false,
                });
            }
        }
        if jobs.is_empty() {
            return None; // no tracking jobs configured, and no TRACK_ENABLED=1 dev fallback
        }
        Some(Cfg { detect_gap: Duration::from_millis(ms), jobs })
    }
}

/// The subset of control's `tracking_config` jsonb the edge cares about — mirrors
/// `sentinel-edge::recording::RecordingConfig`'s shape/fetch style. `#[serde(default)]`
/// everywhere so an older/partial config (or the DB column's bare `{}` default) still parses.
/// The jsonb also carries a legacy `enabled` bool (dead: nothing ever sets it, and job presence
/// in `trackers` is the real enable signal) — intentionally not deserialized here.
#[derive(Debug, Clone, serde::Deserialize)]
struct RemoteConfig {
    #[serde(default)]
    detect_ms: Option<u64>,
    #[serde(default)]
    trackers: Vec<TrackerSpec>,
}

/// One `{camera, class, lines, zone}` job as control reports it — extracted from every enabled
/// graph's `OnEvent("object_entered"/"object_exited"/"line_crossed")` nodes on save (grouped so
/// two nodes naming the same camera+class share one job/uuid-space, their lines merged). `zone`
/// (drawn in the same property panel, a "region of interest" alternative to whole-frame
/// enter/exit), the tuning fields, and `debug` are only meaningful set on ONE of those nodes per
/// camera+class — control keeps whichever it saw last; see `sync_tracking_config`. `#[serde(default
/// = ...)]` on the tuning fields (rather than a bare `#[serde(default)]`, which would zero them)
/// so an older config blob predating this feature still gets sane values, not an instantly-aged-
/// out `max_age: 0`.
#[derive(Debug, Clone, serde::Deserialize)]
struct TrackerSpec {
    camera: String,
    class: String,
    #[serde(default)]
    lines: Vec<Line>,
    #[serde(default)]
    zone: Option<Polygon>,
    #[serde(default)]
    zone_trigger: ZoneTrigger,
    #[serde(default = "default_min_hits")]
    min_hits: u32,
    #[serde(default = "default_max_age")]
    max_age: u32,
    #[serde(default = "default_iou_thresh")]
    iou_thresh: f32,
    #[serde(default = "default_high_thresh")]
    high_thresh: f32,
    #[serde(default = "default_low_thresh")]
    low_thresh: f32,
    #[serde(default)]
    debug: bool,
}

/// `GET /config/tracking` (box-authed) → this box's tenant's tracking config, if reachable.
/// Best-effort like `recording`'s `fetch_config`: `None` on any failure, never fatal.
fn fetch_remote(control_url: &str) -> Option<RemoteConfig> {
    let url = format!("{control_url}/config/tracking");
    let mut req = ureq::AgentBuilder::new().timeout(Duration::from_secs(10)).build().get(&url);
    if let Ok(t) = std::env::var("BOX_TOKEN") {
        let t = t.trim();
        if !t.is_empty() {
            req = req.set("Authorization", &format!("Bearer {t}"));
        }
    }
    match req.call() {
        Ok(resp) => resp.into_json::<RemoteConfig>().ok(),
        Err(e) => {
            log::debug!("tracking config fetch failed (using env-var fallback): {e}");
            None
        }
    }
}

/// Parse `TRACK_LINES` — one or more `name:x1,y1,x2,y2` line defs (normalized `0.0..1.0` frame
/// coords), `;`-separated, e.g. `"gate:0.5,0.0,0.5,1.0;till:0.0,0.9,1.0,0.9"`. A malformed entry
/// is skipped (logged), not fatal — the rest of the config still loads. `camera` is left empty —
/// meaningless here, since the env-var fallback's single job already carries its own `camera`.
fn parse_lines(raw: &str) -> Vec<Line> {
    raw.split(';')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .filter_map(|entry| {
            let (name, coords) = entry.split_once(':')?;
            let nums: Vec<f32> = coords.split(',').filter_map(|n| n.trim().parse().ok()).collect();
            let [x1, y1, x2, y2] = nums[..] else {
                log::warn!("TRACK_LINES: skipping malformed line entry {entry:?}");
                return None;
            };
            Some(Line { camera: String::new(), name: name.trim().to_string(), p1: (x1, y1), p2: (x2, y2) })
        })
        .collect()
}

/// Spawn the tracking manager: every 30s reconcile one worker per (job, resolved camera ip) —
/// a job's `camera == "*"` expands to every camera with a stream, a specific id/ip/name/`"first"`
/// resolves to exactly one. No-op (returns immediately) unless there's at least one tracking job
/// (control's `tracking_config.trackers`, or a `TRACK_ENABLED=1` dev fallback — see [`Cfg::load`]).
/// Spawn the tracking manager: every 30s, RE-FETCH control's tracking config (not just once at
/// boot) and reconcile one worker per (job, resolved camera ip) — a job's `camera == "*"` expands
/// to every camera with a stream, a specific id/ip/name/`"first"` resolves to exactly one. This
/// means enabling a graph, or editing an `OnEvent` node's camera/class/region/tuning, takes
/// effect within ~30s on its own — no edge restart needed (previously `Cfg::load` ran once at
/// boot only, so a running box never noticed a graph change until it restarted).
///
/// The reconcile key is `(job.fingerprint(), ip)`, not `(job index, ip)`: a job's position in
/// the list is stable across reloads as long as the SET of (camera, class) pairs doesn't change
/// (`sync_tracking_config` groups by that key in a `BTreeMap`), so an in-place edit to an
/// EXISTING job (e.g. the user raised `max_age`) would keep the same index and be invisible to
/// an index-keyed diff. Folding the whole job's config into the key means an edit naturally
/// produces a new key — the old worker stops, a new one starts with the fresh config.
pub fn spawn(state: &Arc<AppState>) {
    let state = Arc::clone(state);
    std::thread::spawn(move || {
        // Keyed by (job fingerprint, resolved camera ip).
        let mut running: HashMap<(String, String), Arc<AtomicBool>> = HashMap::new();
        let mut was_disabled = true; // suppress repeating the "disabled" log every 30s
        loop {
            let Some(cfg) = Cfg::load(&state.control_url) else {
                if !was_disabled {
                    log::info!(
                        "object tracking disabled: no tracking jobs configured (wire an OnEvent node for \
                         object_entered/object_exited/line_crossed to a camera in a graph, or set \
                         TRACK_ENABLED=1 for a dev fallback)"
                    );
                }
                was_disabled = true;
                for (_, stop) in running.drain() {
                    stop.store(true, Ordering::Relaxed);
                }
                std::thread::sleep(Duration::from_secs(30));
                continue;
            };
            if was_disabled {
                log::info!("object tracker: manager started ({} job(s), detect every {}ms)", cfg.jobs.len(), cfg.detect_gap.as_millis());
            }
            was_disabled = false;
            let detect_gap = cfg.detect_gap;

            let devices = state.devices.lock().unwrap().clone();
            let desired: Vec<(String, String, Arc<Job>)> = cfg
                .jobs
                .into_iter()
                .flat_map(|job| {
                    let job = Arc::new(job);
                    let fp = job.fingerprint();
                    resolve_job_cameras(&job, &devices).into_iter().map(move |ip| (fp.clone(), ip, Arc::clone(&job))).collect::<Vec<_>>()
                })
                .collect();
            let want: HashSet<(String, String)> = desired.iter().map(|(fp, ip, _)| (fp.clone(), ip.clone())).collect();
            running.retain(|key, stop| {
                let keep = want.contains(key);
                if !keep {
                    stop.store(true, Ordering::Relaxed);
                }
                keep
            });
            for (fp, ip, job) in desired {
                let key = (fp, ip.clone());
                if !running.contains_key(&key) {
                    let stop = Arc::new(AtomicBool::new(false));
                    running.insert(key, Arc::clone(&stop));
                    let st = Arc::clone(&state);
                    std::thread::spawn(move || watch(st, ip, job, detect_gap, stop));
                }
            }
            std::thread::sleep(Duration::from_secs(30));
        }
    });
}

/// Which camera ip(s) a job's `camera` selector currently resolves to, restricted to cameras
/// with a resolvable stream (mirrors the motion manager's `has_stream` gate).
fn resolve_job_cameras(job: &Job, devices: &[app_core::devices::Device]) -> Vec<String> {
    if job.camera == "*" {
        return devices.iter().filter(|d| has_stream(d)).map(|d| d.ip.clone()).collect();
    }
    match app_core::device_actions::resolve_device(devices, &job.camera) {
        Ok(idx) if has_stream(&devices[idx]) => vec![devices[idx].ip.clone()],
        _ => Vec::new(),
    }
}

fn has_stream(d: &app_core::devices::Device) -> bool {
    d.rtsp_url.as_deref().map(|u| !u.trim().is_empty()).unwrap_or(false)
}

/// One (job, camera)'s supervised loop: run the tracker until stopped, retrying on error. Stops
/// either because the job/camera is no longer desired, OR because the job's config changed (see
/// `spawn`'s fingerprint-keyed reconcile) — either way a fresh worker with current config starts
/// right back up if the job's still wanted.
fn watch(state: Arc<AppState>, ip: String, job: Arc<Job>, detect_gap: Duration, stop: Arc<AtomicBool>) {
    log::info!("object tracker[{ip}]: starting ({})", job.class_name);
    while !stop.load(Ordering::Relaxed) {
        run_track(&state, &ip, &job, detect_gap, &stop);
        if stop.load(Ordering::Relaxed) {
            break;
        }
        std::thread::sleep(Duration::from_secs(5));
    }
    // This job is gone (camera/class no longer desired, or its config changed) — drop its stale
    // live-state entry so GET /tracks/:ip doesn't keep reporting long-departed tracks as present.
    state.live_tracks.lock().unwrap().remove(&(ip.clone(), job.class_name.clone()));
    log::info!("object tracker[{ip}]: stopped");
}

/// How long after the last qualifying motion frame the scene still counts as "active". Must
/// comfortably exceed the frame hub's frame period so continuous activity reads as continuous —
/// and it keys off the raw per-frame verdict, never the 4s-throttled motion *emit*, or the gate
/// would stutter in step with the throttle.
const MOTION_HOLD: Duration = Duration::from_secs(3);
/// A fresh worker always detects for this long, whatever motion says — it has to see what was
/// **already static in frame** when it started. A car parked before the box booted never generates
/// motion; without this it would never be detected, and so never enter.
const WARMUP: Duration = Duration::from_secs(5);
/// Even a scene with no motion and nothing tracked runs one detection cycle this often. Cheap
/// insurance (≈1 inference / 30s) against anything that appears without tripping MOG2.
const IDLE_SWEEP: Duration = Duration::from_secs(30);

/// Should this cycle run the detector? YOLOX is by far the most expensive thing on the box, and
/// ungated it costs the same on an empty room at 3am as on a busy shop floor — so skip it when
/// there is provably nothing to see. A skipped cycle also leaves the tracker **untouched**: an
/// un-ticked tracker can't age a track out, which is what we want, since nothing leaves a scene
/// without generating motion.
///
/// - `motion_active` — the scene is moving (raw per-frame MOG2 verdict, never the throttled emit).
/// - `holding_tracks` — any track still alive, **including `Lost` ones**. Two distinct reasons,
///   and both are load-bearing:
///   1. A person who walks in and then stands perfectly still stops producing motion (MOG2 absorbs
///      a stationary subject into the background) but is still there — YOLOX keeps confirming them.
///   2. A track goes `Lost` the moment it stops matching a detection, and only becomes an
///      `object_exited` after `max_age` **unmatched cycles**. Gating on confirmed tracks alone
///      (`MultiTracker::count()`) would close the gate the instant the subject left and starve the
///      tracker of the very cycles it needs to fire the exit — the event would never arrive.
/// - `since_start` — a fresh worker always detects for `WARMUP`, to see what was already static in
///   frame when it started (a car parked before the box booted never generates motion).
/// - `since_last_detect` — the idle sweep; `None` means we've never detected, so do.
fn should_detect(
    gated: bool,
    motion_active: bool,
    holding_tracks: bool,
    since_start: Duration,
    since_last_detect: Option<Duration>,
) -> bool {
    !gated
        || motion_active
        || holding_tracks
        || since_start < WARMUP
        || since_last_detect.map(|d| d >= IDLE_SWEEP).unwrap_or(true)
}

fn run_track(state: &Arc<AppState>, ip: &str, job: &Job, detect_gap: Duration, stop: &AtomicBool) {
    let mut tracker = MultiTracker::new(job.tracker_config());
    // Frames come from the per-camera hub: fetched once, decoded once, shared with motion and with
    // every other tracking job on this camera (see `crate::frames`).
    let mut frames = crate::frames::subscribe(ip);
    // Verbose algorithm-internals logging: on if EITHER this job's own graph has its "Debug"
    // toggle on (`Graph.debug`, synced onto the job's `debug` field — see `sync_tracking_config`
    // and `TrackerSpec`), OR the box-wide `TRACK_DEBUG` env var is set. Never sent to control —
    // these are diagnostic-only lines the box's own log stream carries (see `TrackEvent`'s
    // `Lost`/`Revived`/`Removed` variants below).
    let env_debug = std::env::var("TRACK_DEBUG").map(|v| matches!(v.trim(), "1" | "true" | "yes")).unwrap_or(false);
    let debug = job.debug || env_debug;
    // The motion gate is the whole point of the hub: YOLOX is by far the most expensive thing on
    // the box, and an empty room used to cost exactly as much as a busy one. `TRACK_MOTION_GATE=0`
    // restores the old always-on behaviour for a scenario we haven't thought of.
    let gated = std::env::var("TRACK_MOTION_GATE").map(|v| !matches!(v.trim(), "0" | "false" | "no")).unwrap_or(true);
    let started = std::time::Instant::now();
    let mut last_detect: Option<std::time::Instant> = None;

    while !stop.load(Ordering::Relaxed) {
        let tick = std::time::Instant::now();
        // Newest frame this worker hasn't seen. A timeout means the camera is down or warming —
        // loop so the stop flag is re-checked.
        let Some(frame) = frames.next(Duration::from_millis(500)) else {
            continue;
        };

        // ---- the motion gate (see `should_detect`) ------------------------------------------
        let motion_active = frame.motion_at.map(|t| t.elapsed() < MOTION_HOLD).unwrap_or(false);
        let (tentative, confirmed, lost) = tracker.state_counts();
        let holding_tracks = tentative + confirmed + lost > 0;
        if !should_detect(
            gated,
            motion_active,
            holding_tracks,
            started.elapsed(),
            last_detect.map(|t| t.elapsed()),
        ) {
            if let Some(rem) = detect_gap.checked_sub(tick.elapsed()) {
                std::thread::sleep(rem);
            }
            continue;
        }
        last_detect = Some(tick);

        // YOLOX on the hub's already-decoded BGR buffer (pure tract, no OpenCV, no second JPEG
        // decode), filtered to this job's tracked class.
        let dets: Vec<Detection> = match detector::detect_bgr(&frame.bgr, frame.w as u32, frame.h as u32) {
            Ok(v) => v.into_iter().filter(|d| d.class == job.class_id).collect(),
            Err(e) => {
                log::debug!("object tracker[{ip}]: detect failed: {e}");
                std::thread::sleep(detect_gap);
                continue;
            }
        };
        let now_ms = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
        // Lines/zone need the frame's pixel size to scale their normalized coords — the hub already
        // knows it, so this no longer costs a second decode of the frame just to read its header.
        let events = if job.lines.is_empty() && job.zone.is_none() {
            tracker.update(&dets, now_ms)
        } else {
            tracker.update_full(
                &dets,
                now_ms,
                (frame.w as f32, frame.h as f32),
                &job.lines,
                job.zone.as_ref().map(|z| (z, job.zone_trigger)),
            )
        };
        let count = tracker.count() as i64;
        if debug {
            let (tentative, confirmed, lost) = tracker.state_counts();
            log::info!(
                "object tracker[{ip}]: {} det, {} tracked ({tentative} tentative, {confirmed} confirmed, {lost} lost), {} event(s)",
                dets.len(),
                count,
                events.len()
            );
        }
        // Publish this cycle's "state of the world" for GET /tracks/:ip (the GetTrackedObjects
        // node's backing store) — this job's own key only, never touching another job's entry.
        let snapshot: Vec<crate::state::LiveTrackOut> = tracker
            .confirmed()
            .map(|t| crate::state::LiveTrackOut {
                uuid: t.uuid.to_string(),
                class: job.class_name.clone(),
                bbox: to_i32(t.bbox()),
                zone: match (&job.zone, t.zone_inside()) {
                    (Some(z), Some(true)) => z.name.clone(),
                    _ => String::new(),
                },
            })
            .collect();
        state.live_tracks.lock().unwrap().insert((ip.to_string(), job.class_name.clone()), snapshot);
        for ev in events {
            // When a zone is configured, `object_entered`/`object_exited` mean entering/leaving
            // THAT polygon (see `TrackEvent::ZoneEntered`/`ZoneExited` below) — the whole-frame
            // lifecycle events are suppressed so the job only ever reports one enter/exit story,
            // not both. Without a zone, whole-frame events are exactly what's pushed, as before.
            let out = match ev {
                TrackEvent::Entered { uuid, bbox, .. } if job.zone.is_none() => TrackEventOut {
                    kind: "object_entered".into(),
                    camera_ip: ip.to_string(),
                    uuid: uuid.to_string(),
                    class: job.class_name.clone(),
                    bbox: to_i32(bbox),
                    count,
                    dwell_secs: None,
                    line: None,
                    direction: None,
                    zone: None,
                },
                TrackEvent::Exited { uuid, bbox, dwell_ms, .. } if job.zone.is_none() => TrackEventOut {
                    kind: "object_exited".into(),
                    camera_ip: ip.to_string(),
                    uuid: uuid.to_string(),
                    class: job.class_name.clone(),
                    bbox: to_i32(bbox),
                    count,
                    dwell_secs: Some((dwell_ms / 1000) as i64),
                    line: None,
                    direction: None,
                    zone: None,
                },
                TrackEvent::Entered { .. } | TrackEvent::Exited { .. } => continue, // zone job: whole-frame events suppressed
                TrackEvent::Crossed { uuid, line, direction, .. } => TrackEventOut {
                    kind: "line_crossed".into(),
                    camera_ip: ip.to_string(),
                    uuid: uuid.to_string(),
                    class: job.class_name.clone(),
                    bbox: [0; 4], // not meaningful for a crossing — the uuid ties it back to the entity
                    count,
                    dwell_secs: None,
                    line: Some(line),
                    direction: Some(direction.as_str().to_string()),
                    zone: None,
                },
                TrackEvent::ZoneEntered { uuid, zone, .. } => TrackEventOut {
                    kind: "object_entered".into(),
                    camera_ip: ip.to_string(),
                    uuid: uuid.to_string(),
                    class: job.class_name.clone(),
                    bbox: [0; 4], // the tracker doesn't recompute a fresh bbox for a zone event
                    count,
                    dwell_secs: None,
                    line: None,
                    direction: None,
                    zone: Some(zone),
                },
                TrackEvent::ZoneExited { uuid, zone, dwell_ms, .. } => TrackEventOut {
                    kind: "object_exited".into(),
                    camera_ip: ip.to_string(),
                    uuid: uuid.to_string(),
                    class: job.class_name.clone(),
                    bbox: [0; 4],
                    count,
                    dwell_secs: Some((dwell_ms / 1000) as i64),
                    line: None,
                    direction: None,
                    zone: Some(zone),
                },
                // Diagnostic-only: internal state transitions, never relayed to control as a
                // domain event — logged only when this job's debug flag (or TRACK_DEBUG) is on.
                TrackEvent::Lost { uuid, .. } => {
                    if debug {
                        log::info!("object tracker[{ip}]: track {uuid} lost (unmatched this cycle)");
                    }
                    continue;
                }
                TrackEvent::Revived { uuid, cycles_lost, .. } => {
                    if debug {
                        log::info!("object tracker[{ip}]: track {uuid} revived (was lost {cycles_lost} cycle(s))");
                    }
                    continue;
                }
                TrackEvent::Removed { uuid, cycles_survived, was_confirmed, .. } => {
                    if debug {
                        log::info!(
                            "object tracker[{ip}]: track {uuid} removed (unmatched {cycles_survived} cycle(s), was_confirmed={was_confirmed})"
                        );
                    }
                    continue;
                }
            };
            log::info!("object tracker[{ip}]: {} {} (count={count})", out.kind, out.uuid);
            state.push_track_event(&out);
        }
        // Pace to the detection cadence (detection itself costs tens of ms).
        if let Some(rem) = detect_gap.checked_sub(tick.elapsed()) {
            std::thread::sleep(rem);
        }
    }
}

fn to_i32(b: [f32; 4]) -> [i32; 4] {
    [b[0] as i32, b[1] as i32, b[2] as i32, b[3] as i32]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Long enough that neither WARMUP nor the idle sweep is what's answering.
    fn settled() -> (Duration, Option<Duration>) {
        (WARMUP + Duration::from_secs(1), Some(Duration::from_secs(1)))
    }

    #[test]
    fn idle_scene_skips_the_detector() {
        let (since_start, since_detect) = settled();
        assert!(!should_detect(true, false, false, since_start, since_detect));
    }

    #[test]
    fn motion_opens_the_gate() {
        let (since_start, since_detect) = settled();
        assert!(should_detect(true, true, false, since_start, since_detect));
    }

    /// The exit-correctness case. A subject who has left is a `Lost` track with no motion behind it
    /// — and it needs `max_age` more *unmatched detection cycles* to become an `object_exited`. If
    /// the gate closed here (as it would on confirmed-tracks-only), that event would never fire.
    #[test]
    fn a_lost_track_keeps_the_gate_open_so_the_exit_can_fire() {
        let (since_start, since_detect) = settled();
        assert!(should_detect(true, false, true, since_start, since_detect));
    }

    /// A person standing perfectly still is absorbed into MOG2's background — no motion, but they
    /// are still there and YOLOX still sees them.
    #[test]
    fn a_still_subject_keeps_the_gate_open() {
        let (since_start, since_detect) = settled();
        assert!(should_detect(true, false, true, since_start, since_detect));
    }

    #[test]
    fn a_fresh_worker_detects_whatever_was_already_in_frame() {
        assert!(should_detect(true, false, false, Duration::from_secs(1), Some(Duration::from_secs(1))));
    }

    #[test]
    fn the_idle_sweep_runs_a_cycle_eventually() {
        let since_start = WARMUP + Duration::from_secs(1);
        assert!(should_detect(true, false, false, since_start, Some(IDLE_SWEEP)));
        assert!(should_detect(true, false, false, since_start, None));
    }

    #[test]
    fn ungated_always_detects() {
        let (since_start, since_detect) = settled();
        assert!(should_detect(false, false, false, since_start, since_detect));
    }
}
