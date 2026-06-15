//! Warm in-memory frame cache — ~zero-latency "current frame" reads.
//!
//! A workflow that recognises people every second shouldn't pay a camera/go2rtc round
//! trip per tick. The process-wide [`FrameHub`] keeps one lightweight background grabber
//! per camera that continuously pulls the latest JPEG from the warm go2rtc feed into
//! memory; a `current()` read then returns the most-recent frame instantly (a memory
//! clone, no network). Grabbers start on demand (first read for a camera) and self-stop
//! after an idle period with no reads, so we never poll cameras nobody is watching.
//!
//! This is the fast path behind the `GetCurrentFrame` workflow node. `CameraSnapshot`
//! stays the on-demand path (best freshness/quality, may connect a cold camera).
//!
//! **Scope: one hub per pod**, NOT per graph-run and NOT a process-global. A 1 Hz graph
//! spawns a fresh run each tick; a per-run hub would start cold every tick and defeat the
//! cache. So the pod (the sentinel-flow instance / the desktop app) owns ONE [`FrameHub`]
//! at boot and shares it across all of its runs, graphs, and concurrent instances of the
//! same graph — keyed by camera IP, so they share one grabber per camera. Separate pods
//! each keep their own hub; go2rtc multiplexes one RTSP feed to all of them.
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

/// How often a grabber refreshes its camera's latest frame (~5 fps). Fast enough that a
/// 1 Hz consumer always sees a sub-300 ms-old frame, cheap enough to leave running.
const REFRESH: Duration = Duration::from_millis(200);
/// Stop a grabber when nobody has read its frame for this long (no active consumer).
const IDLE_STOP: Duration = Duration::from_secs(15);

struct Slot {
    latest: RwLock<Option<(Instant, Arc<Vec<u8>>)>>,
    last_read: RwLock<Instant>,
    running: AtomicBool,
}

/// One background grabber per camera, keyed by camera IP. Created once per pod (see the
/// module docs) and shared via `Arc` across all of that pod's graph runs.
pub struct FrameHub {
    cams: Mutex<HashMap<String, Arc<Slot>>>,
}

impl FrameHub {
    /// Create a pod's frame hub. Wrap it in an `Arc`, hold it in the pod's long-lived
    /// state, and share clones into every run's frame source — do NOT create one per
    /// graph run (that would start cold every tick and defeat the cache).
    pub fn new() -> FrameHub {
        FrameHub { cams: Mutex::new(HashMap::new()) }
    }

    /// The most-recent frame for `ip` no older than `max_age`, with its age — starting a
    /// warm grabber if one isn't running. `None` until the first frame lands (or if the
    /// latest is staler than `max_age`); callers should fall back to an on-demand
    /// snapshot then. Records the read so the grabber stays alive while in use.
    pub fn current(&self, ip: &str, max_age: Duration) -> Option<(Arc<Vec<u8>>, Duration)> {
        let slot = self.ensure(ip);
        *slot.last_read.write().unwrap() = Instant::now();
        let g = slot.latest.read().unwrap();
        match &*g {
            Some((t, bytes)) if t.elapsed() <= max_age => Some((bytes.clone(), t.elapsed())),
            _ => None,
        }
    }

    /// Pre-warm a camera's grabber without reading (e.g. when a graph using it starts).
    pub fn warm(&self, ip: &str) {
        let _ = self.ensure(ip);
    }

    /// Get the slot for `ip`, starting (or restarting an idle-stopped) grabber.
    fn ensure(&self, ip: &str) -> Arc<Slot> {
        let mut cams = self.cams.lock().unwrap();
        if let Some(s) = cams.get(ip) {
            // Restart if a prior grabber self-stopped on idle (swap returns the old flag).
            if !s.running.swap(true, Ordering::SeqCst) {
                spawn_grabber(ip.to_string(), s.clone());
            }
            return s.clone();
        }
        let slot = Arc::new(Slot {
            latest: RwLock::new(None),
            last_read: RwLock::new(Instant::now()),
            running: AtomicBool::new(true),
        });
        cams.insert(ip.to_string(), slot.clone());
        spawn_grabber(ip.to_string(), slot.clone());
        slot
    }
}

impl Default for FrameHub {
    fn default() -> Self {
        FrameHub::new()
    }
}

/// Background loop: pull the latest JPEG from the warm go2rtc feed into `slot` at
/// `REFRESH`, until no consumer has read for `IDLE_STOP`.
fn spawn_grabber(ip: String, slot: Arc<Slot>) {
    std::thread::spawn(move || loop {
        if slot.last_read.read().unwrap().elapsed() > IDLE_STOP {
            slot.running.store(false, Ordering::SeqCst);
            return; // a later read restarts us via FrameHub::ensure
        }
        match crate::go2rtc::frame(&ip) {
            Ok(bytes) => *slot.latest.write().unwrap() = Some((Instant::now(), Arc::new(bytes))),
            Err(_) => { /* stream cold/unavailable — keep retrying at the slow cadence */ }
        }
        std::thread::sleep(REFRESH);
    });
}
