//! On-demand clip recording with an in-memory H264 pre-roll ring buffer.
//!
//! **Ring buffer** (`start_ring`)
//!   A background thread per camera opens an RTSP connection to go2rtc's local
//!   re-server (`rtsp://127.0.0.1:8554/cam_{ip_key}`). We use the `retina` crate
//!   ONLY for RTSP transport + RTP ordering (`PacketItem::Rtp`) and do our own
//!   lenient H264 depacketization (`H264Reassembler`) — retina's built-in codec
//!   layer rejects go2rtc's repackaged NALs with "forbidden sequence 00 00 00".
//!   Reassembled Annex-B access units are kept in a `VecDeque<H264Frame>` in
//!   memory (the *mem-ring*). No segments are written to disk between recordings.
//!
//! **Recording** (`start` / `stop`)
//!   `start()` snapshots the mem-ring from the last IDR frame (guarantees a clean
//!   keyframe — no black frames) and installs the live sink under the SAME ring
//!   lock, so no frame is dropped or duplicated at the boundary. It starts a muxer
//!   ([`start_muxer`] — ffmpeg, or the linked GStreamer one on a `gstreamer-linked`
//!   box, card #138 A2), writes the pre-roll FRAMES, then a relay thread feeds
//!   subsequent live frames to it. Frames carry their capture time so the GStreamer
//!   muxer can stamp a real PTS per buffer.
//!
//!   `stop()` clears the live sink → channel closes → relay drains → the sink closes
//!   (ffmpeg stdin EOF / appsrc EOS) → the muxer writes the trailer. No concat step.
//!   The clip's duration comes from the captured frame span, not `ffprobe` (A3).

use futures::StreamExt as _;
use retina::client::PacketItem;
use std::collections::{HashMap, VecDeque};
use std::fs;
use std::io::Write as _;
use std::path::PathBuf;
use std::process::{Child, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

// ---- friendly name word list ------------------------------------------------

const ADJECTIVES: &[&str] = &[
    "amber", "azure", "bold", "bright", "calm", "cedar", "clear", "cloud", "coral", "crisp",
    "dark", "dawn", "deep", "deft", "dense", "drift", "dusk", "early", "faint", "fast", "fern",
    "firm", "flash", "fleet", "frost", "gentle", "ghost", "glad", "glide", "gold", "grand", "grey",
    "gust", "hardy", "haze", "high", "hollow", "iron", "jade", "keen", "kind", "lake", "lean",
    "light", "lofty", "lone", "loud", "mist", "misty", "moss", "muted", "night", "noble", "north",
    "oak", "pale", "peak", "pine", "plain", "prime", "pure", "quick", "quiet", "rapid", "raw",
    "reef", "rich", "ridge", "ripple", "rise", "roam", "rocky", "rough", "sage", "sharp", "silver",
    "sleek", "slim", "slow", "smooth", "soft", "solid", "stark", "steady", "steel", "still",
    "stone", "storm", "strong", "swift", "tall", "thin", "tidal", "trim", "true", "vast", "veil",
    "warm", "wide", "wild",
];

const NOUNS: &[&str] = &[
    "arc", "ash", "bay", "beam", "birch", "blade", "blaze", "bloom", "boulder", "branch", "bridge",
    "brook", "canyon", "cedar", "cliff", "cloud", "comet", "cove", "creek", "crest", "dawn",
    "delta", "drift", "dune", "dusk", "echo", "ember", "falcon", "fern", "field", "fjord", "flame",
    "flare", "flash", "flint", "fog", "forest", "gale", "gate", "glacier", "glen", "grove", "gust",
    "harbor", "hawk", "haze", "hill", "hollow", "horizon", "isle", "jade", "lake", "ledge",
    "light", "loch", "mesa", "meteor", "mist", "moon", "moss", "mount", "nebula", "night", "north",
    "oak", "orbit", "pass", "peak", "pine", "plain", "pond", "pulse", "quarry", "rain", "range",
    "rapid", "reef", "ridge", "rift", "river", "rock", "root", "run", "sage", "salt", "sand",
    "shore", "sierra", "sky", "slate", "snow", "sol", "spark", "spire", "spring", "star", "stone",
    "storm", "stream", "summit", "sun", "surge", "tide", "timber", "torch", "trail", "vale",
    "vault", "wave", "wind",
];

pub fn generate_friendly_name(epoch: i64) -> String {
    let e = epoch.unsigned_abs() as usize;
    let na = ADJECTIVES.len();
    let nn = NOUNS.len();
    let w1 = ADJECTIVES[e % na];
    let w2 = ADJECTIVES[(e.wrapping_mul(31)) % na];
    let w3 = NOUNS[(e.wrapping_mul(97)) % nn];
    format!("{w1}-{w2}-{w3}")
}

// ---- paginated list ---------------------------------------------------------

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ClipListItem {
    pub clip_id: String,
    pub friendly_name: String,
    pub camera_ip: String,
    pub duration_secs: i32,
    pub size_bytes: i64,
    pub started_at: String,
}

#[derive(Debug, serde::Serialize)]
pub struct ClipPage {
    pub total: usize,
    pub items: Vec<ClipListItem>,
}

#[derive(Debug)]
pub struct ListParams {
    pub camera_ip: Option<String>,
    pub sort: String,
    pub dir: String,
    pub limit: usize,
    pub offset: usize,
    pub from: Option<String>,
    pub to: Option<String>,
}

// ---- config -----------------------------------------------------------------

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct RecordingConfig {
    #[serde(default = "default_pre_roll")]
    pub pre_roll_secs: u32,
    #[serde(default = "default_storage")]
    pub max_storage_mb: u64,
    #[serde(default = "default_codec")]
    pub codec: String,
    #[serde(default)]
    pub cameras_always_warm: bool,
    /// Which camera stream feeds face capture (#120): "preview" (low-res substream, default) or
    /// "hd" (the main/HD stream — sharper faces, heavier decode; gate on a GPU-decode-capable box).
    #[serde(default = "default_capture_stream")]
    pub face_capture_stream: String,
    // ---- MOG2 motion detection (per-box, UI-tunable) — see `motion.rs` `Params`. Each defaults to
    //      the same value `Params::from_env` uses, so an older config (missing these) behaves exactly
    //      as before. The per-box config is now authoritative; the `MOTION_*` env vars are a fallback.
    #[serde(default = "default_motion_var")]
    pub motion_var_threshold: f64,
    /// Frames/second sampled into MOG2 (heavier = more responsive, costs more decode). Default 4.
    #[serde(default = "default_motion_fps")]
    pub motion_fps: u32,
    /// % of the frame that must change to count as motion (lower = more sensitive). Default 0.5.
    #[serde(default = "default_motion_floor")]
    pub motion_floor_pct: f64,
    /// Minimum seconds between motion events (debounce). Default 4.
    #[serde(default = "default_motion_interval")]
    pub motion_min_interval_secs: u64,
    /// Minimum blob area (in processing pixels) to qualify as motion. Default 300.
    #[serde(default = "default_motion_min_area")]
    pub motion_min_area: f64,
    /// Processing resolution the frame is downscaled to before MOG2. Default 480×270.
    #[serde(default = "default_motion_proc_w")]
    pub motion_proc_w: u32,
    #[serde(default = "default_motion_proc_h")]
    pub motion_proc_h: u32,
    /// MOG2 background-model history, in FRAMES (adaptation window ≈ history / fps). Default 333.
    #[serde(default = "default_motion_history")]
    pub motion_history: i32,
    #[serde(default = "default_recordings_path")]
    pub recordings_path: String,
    /// Delete a clip's local file as soon as R2 confirms it's durable, making this box's disk a
    /// transient spool instead of a growing archive (card #78). Off by default — and it must stay
    /// off for a tenant whose plan sells no cloud recording, where the local file is the ONLY copy.
    #[serde(default)]
    pub delete_local_after_upload: bool,
    /// Which media tool the edge shells out to for camera decode (card #138): `"ffmpeg"` (default)
    /// or `"gstreamer"`. A coexistence flag, NOT a permanent fork — see
    /// `docs/plans/edge-gstreamer-backend.md`. `"gstreamer-linked"` (linked bindings, the
    /// successor) additionally drives the recording mux (A2) and the cold snapshot (A5); plain
    /// `"gstreamer"` is the older subprocess path and still only affects `capture.rs`'s A1 decode.
    /// Falls back to ffmpeg at read time if `gst-launch-1.0` isn't actually on this box's PATH
    /// (`state::gstreamer_ok()`), so a stale/mistaken flag never breaks capture.
    #[serde(default = "default_media_backend")]
    pub media_backend: String,
}

impl Default for RecordingConfig {
    fn default() -> Self {
        Self {
            pre_roll_secs: 10,
            max_storage_mb: 1000,
            codec: "copy".to_string(),
            cameras_always_warm: false,
            face_capture_stream: default_capture_stream(),
            motion_var_threshold: 16.0,
            motion_fps: default_motion_fps(),
            motion_floor_pct: default_motion_floor(),
            motion_min_interval_secs: default_motion_interval(),
            motion_min_area: default_motion_min_area(),
            motion_proc_w: default_motion_proc_w(),
            motion_proc_h: default_motion_proc_h(),
            motion_history: default_motion_history(),
            recordings_path: default_recordings_path(),
            delete_local_after_upload: false,
            media_backend: default_media_backend(),
        }
    }
}

fn default_motion_var() -> f64 {
    16.0
}
fn default_motion_fps() -> u32 {
    4
}
fn default_motion_floor() -> f64 {
    0.5
}
fn default_motion_interval() -> u64 {
    4
}
fn default_motion_min_area() -> f64 {
    300.0
}
fn default_motion_proc_w() -> u32 {
    480
}
fn default_motion_proc_h() -> u32 {
    270
}
fn default_motion_history() -> i32 {
    333
}
fn default_pre_roll() -> u32 {
    10
}
fn default_storage() -> u64 {
    1000
}
fn default_codec() -> String {
    "copy".to_string()
}
fn default_capture_stream() -> String {
    "preview".to_string()
}
fn default_recordings_path() -> String {
    crate::paths::recordings_dir()
        .to_string_lossy()
        .into_owned()
}
fn default_media_backend() -> String {
    // Card #138: GStreamer is the first-class backend now. A build that LINKS it (the `gst-linked`
    // feature — macOS/iOS today, every appliance once the Linux static-musl → glibc move lands)
    // defaults to it with no config or env needed; a build without it (the Linux musl release, for
    // now) falls back to ffmpeg so it still has a media path.
    #[cfg(feature = "gst-linked")]
    {
        "gstreamer-linked".to_string()
    }
    #[cfg(not(feature = "gst-linked"))]
    {
        "ffmpeg".to_string()
    }
}

// ---- storage stats ----------------------------------------------------------

#[derive(Debug, Clone, serde::Serialize)]
pub struct StorageStats {
    pub used_bytes: u64,
    pub max_bytes: u64,
    pub clip_count: usize,
    pub path: String,
    pub pct: u8,
}

impl StorageStats {
    pub fn compute(dir: &std::path::Path, max_mb: u64) -> Self {
        let mut used_bytes: u64 = 0;
        let mut clip_count: usize = 0;
        if let Ok(rd) = fs::read_dir(dir) {
            for entry in rd.filter_map(|e| e.ok()) {
                let p = entry.path();
                if p.extension().map(|x| x == "json").unwrap_or(false) {
                    if let Ok(raw) = fs::read_to_string(&p) {
                        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw) {
                            used_bytes += v.get("size_bytes").and_then(|b| b.as_u64()).unwrap_or(0);
                            clip_count += 1;
                        }
                    }
                }
            }
        }
        let max_bytes = max_mb.saturating_mul(1024 * 1024);
        let pct = if max_bytes == 0 {
            100
        } else {
            ((used_bytes * 100) / max_bytes).min(100) as u8
        };
        StorageStats {
            used_bytes,
            max_bytes,
            clip_count,
            path: dir.to_string_lossy().into_owned(),
            pct,
        }
    }
}

// ---- in-memory ring buffer --------------------------------------------------

struct H264Frame {
    data: Vec<u8>, // H264 Annex-B bytes
    is_keyframe: bool,
    /// Monotonic arrival time. Pre-roll is selected by THIS, not by frame count — a count-based
    /// window can't tell "10 seconds ago" from "the tail of yesterday" left in the ring after a
    /// disconnect, and would splice stale footage onto a new clip (#101). Monotonic so a wall-clock
    /// jump can't corrupt the window.
    at: std::time::Instant,
}

struct MemRing {
    frames: VecDeque<H264Frame>,
    /// When a recording is active, the collector forwards each new access unit
    /// here. Installed by `start()` and cleared by `stop()` under this same lock,
    /// so the pre-roll/live boundary has no gap or overlap.
    // Carries the frame bytes AND their capture time — the mp4 muxer needs a real PTS per
    // frame (card #138 A2); an assumed framerate would yield clips that play at the wrong speed.
    live_sink: Option<std::sync::mpsc::SyncSender<(Vec<u8>, std::time::Instant)>>,
}

// ---- ring session -----------------------------------------------------------

/// Per-camera ring state. The collector thread owns the RTSP connection and
/// writes reassembled access units into `mem_ring`. During a recording it also
/// forwards frames via `mem_ring.live_sink` so they reach the active ffmpeg.
struct RingSession {
    #[allow(dead_code)]
    stop_tx: std::sync::mpsc::SyncSender<()>,
    mem_ring: Arc<Mutex<MemRing>>,
}

// ---- active recording -------------------------------------------------------

struct Session {
    clip_id: String,
    camera_ip: String,
    output: PathBuf,
    started_at: String,
    relay_thread: Option<std::thread::JoinHandle<()>>,
    muxer: Muxer,
    /// First and last frame capture times actually written to this clip (card #138 A3). The clip's
    /// duration is `last - first` — the real footage span — which needs no `ffprobe` and beats the
    /// wall-clock fallback (that one counts time the recorder spent waiting for an IDR, and any
    /// gap where the camera dropped out, as if it were footage).
    span: Arc<Mutex<Option<(std::time::Instant, std::time::Instant)>>>,
}

/// Record a frame's capture time into a clip's span (first one opens it, every one extends it).
fn note_span(
    span: &Mutex<Option<(std::time::Instant, std::time::Instant)>>,
    at: std::time::Instant,
) {
    let mut g = span.lock().unwrap();
    match &mut *g {
        Some((_, last)) => *last = at,
        None => *g = Some((at, at)),
    }
}

/// Spawn the mp4 muxer for a recording (A2): reads raw Annex-B H264 from stdin, remuxes
/// (no re-encode — same as ffmpeg's `-c:v copy`) into `output`.
///
/// **ffmpeg-only for now, even when `media_backend=gstreamer`** — card #138's GStreamer path was
/// tried and rejected here: `fdsrc fd=0 ! h264parse ! mp4mux ! filesink` fails with "Buffer has no
/// PTS" for a raw elementary stream fed over stdin, on both `filesrc` (a real file, ruling out a
/// pipe-specific issue) and `fdsrc` (with `do-timestamp=true`, tried with/without an explicit
/// `framerate` cap) — confirmed live against a real RTSP stream captured on the Jetson test box.
/// `h264parse`/`mp4mux` need real PTS to remux, which a subprocess-only pipeline (no `appsrc`)
/// can't hand-assign; fixing it needs `gstreamer-rs` bindings, which #79 rules out (the edge
/// links nothing). See `docs/plans/edge-gstreamer-backend.md` (Phase 2b, blocked).
/// The running muxer a session owns; `wait()` blocks until the file is finalised.
enum Muxer {
    Ffmpeg(Child),
    #[cfg(feature = "gst-linked")]
    Gst(crate::gst_mux::GstMuxer),
}

impl Muxer {
    fn wait(&mut self) {
        match self {
            Muxer::Ffmpeg(c) => {
                let _ = c.wait();
            }
            #[cfg(feature = "gst-linked")]
            Muxer::Gst(m) => m.wait(),
        }
    }
}

/// The write end handed to the relay thread. Both backends take **one complete access unit per
/// call** plus its capture time; ffmpeg ignores the time (it re-derives timing itself), GStreamer
/// uses it as the buffer PTS — which is the whole reason A2 was blocked until linked bindings.
enum MuxSink {
    Ffmpeg(std::process::ChildStdin),
    #[cfg(feature = "gst-linked")]
    Gst(crate::gst_mux::GstSink),
}

impl MuxSink {
    fn write_frame(&mut self, data: &[u8], _at: std::time::Instant) -> std::io::Result<()> {
        match self {
            MuxSink::Ffmpeg(w) => w.write_all(data),
            #[cfg(feature = "gst-linked")]
            MuxSink::Gst(s) => s.write_frame(data, _at),
        }
    }

    /// Close the stream so the muxer writes its trailer (ffmpeg: stdin EOF; GStreamer: EOS).
    fn finish(self) {
        match self {
            MuxSink::Ffmpeg(w) => drop(w),
            #[cfg(feature = "gst-linked")]
            MuxSink::Gst(s) => s.finish(),
        }
    }
}

/// Start the muxer for a clip. GStreamer only on a box actually running the linked backend;
/// everything else keeps the ffmpeg path byte-for-byte.
fn start_muxer(
    output: &std::path::Path,
    _backend: crate::media::Backend,
) -> Result<(Muxer, MuxSink), String> {
    #[cfg(feature = "gst-linked")]
    if _backend == crate::media::Backend::GstreamerLinked {
        let (m, s) = crate::gst_mux::start(output)?;
        return Ok((Muxer::Gst(m), MuxSink::Gst(s)));
    }
    let mut child = spawn_muxer(output).map_err(|e| format!("mux spawn: {e}"))?;
    let stdin = child.stdin.take().ok_or("muxer stdin missing")?;
    Ok((Muxer::Ffmpeg(child), MuxSink::Ffmpeg(stdin)))
}

fn spawn_muxer(output: &std::path::Path) -> std::io::Result<Child> {
    crate::proc::command("ffmpeg")
        .args([
            "-loglevel",
            "warning",
            "-f",
            "h264",
            "-i",
            "pipe:0",
            "-c:v",
            "copy",
            "-movflags",
            "+faststart",
            "-y",
            output.to_str().unwrap_or_default(),
        ])
        .stdin(Stdio::piped())
        .spawn()
}

// ---- manager ----------------------------------------------------------------

pub struct RecordingManager {
    pub config: RecordingConfig,
    dir: PathBuf,
    sessions: Mutex<HashMap<String, Session>>,
    ring_sessions: Arc<Mutex<HashMap<String, RingSession>>>,
    /// camera ip → the RTSP URL to dial for its ring (card #138, G5). Published by
    /// [`crate::state::AppState`], which is what holds the devices and their credentials; empty for
    /// a camera we cannot dial ourselves, and then the ring falls back to go2rtc's re-serve.
    dials: Mutex<HashMap<String, String>>,
}

impl RecordingManager {
    pub fn boot(control_url: &str) -> Arc<RecordingManager> {
        let mut config = fetch_config(control_url).unwrap_or_default();
        // Local override for the media backend (card #138) — lets a single test box force
        // "gstreamer" without waiting on a per-box control-side toggle, mirroring how
        // EDGE_RECORDING_DIR overrides the pulled path below.
        if let Ok(mb) = std::env::var("EDGE_MEDIA_BACKEND") {
            config.media_backend = mb;
        }
        let dir = std::env::var("EDGE_RECORDING_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(&config.recordings_path));
        if let Err(e) = fs::create_dir_all(&dir) {
            log::warn!("recording: mkdir {:?}: {e}", dir);
        }
        log::info!(
            "Recording dir {:?}, pre_roll={}s, always_warm={}, media_backend={}",
            dir,
            config.pre_roll_secs,
            config.cameras_always_warm,
            config.media_backend
        );
        Arc::new(RecordingManager {
            config,
            dir,
            sessions: Mutex::new(HashMap::new()),
            ring_sessions: Arc::new(Mutex::new(HashMap::new())),
            dials: Mutex::new(HashMap::new()),
        })
    }

    // ---- ring ---------------------------------------------------------------

    /// Publish the per-camera dial URLs. Called wherever the device set is reconciled, so a camera
    /// that changes address or has its credentials re-staged is dialled correctly on its next ring.
    pub fn set_dials(&self, dials: HashMap<String, String>) {
        *self.dials.lock().unwrap() = dials;
    }

    pub fn start_ring(&self, camera_ip: &str) {
        {
            if self.ring_sessions.lock().unwrap().contains_key(camera_ip) {
                return;
            }
        }

        let (stop_tx, stop_rx) = std::sync::mpsc::sync_channel(1);
        let mem_ring = Arc::new(Mutex::new(MemRing {
            frames: VecDeque::new(),
            live_sink: None,
        }));

        let ip_t = camera_ip.to_string();
        let pre_roll_secs = self.config.pre_roll_secs;
        let mem_ring_t = Arc::clone(&mem_ring);
        let dial = self.dials.lock().unwrap().get(camera_ip).cloned();

        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("tokio rt for RTSP ring");
            rt.block_on(run_rtsp_collector(
                ip_t,
                dial,
                pre_roll_secs,
                mem_ring_t,
                stop_rx,
            ));
        });

        self.ring_sessions
            .lock()
            .unwrap()
            .insert(camera_ip.to_string(), RingSession { stop_tx, mem_ring });

        log::info!(
            "ring[{camera_ip}]: started (RTSP raw-RTP, {}s in-memory pre-roll)",
            pre_roll_secs
        );
    }

    pub fn stop_ring(&self, camera_ip: &str) {
        if let Some(rs) = self.ring_sessions.lock().unwrap().remove(camera_ip) {
            let _ = rs.stop_tx.try_send(());
        }
    }

    /// Stop every ring whose camera is no longer in `keep` — the fix for rings that outlive their
    /// camera (#98). A camera changing IP, being removed, or going un-streamable left its old ring
    /// retrying a dead address forever (DESCRIBE 404 every ~3s), because `start_ring` only ever adds
    /// and nothing subtracted. Called on every reload, so `ring_sessions` mirrors the device set
    /// instead of growing without bound.
    pub fn reconcile_rings(&self, keep: &std::collections::HashSet<String>) {
        let orphans: Vec<String> = self
            .ring_sessions
            .lock()
            .unwrap()
            .keys()
            .filter(|ip| !keep.contains(*ip))
            .cloned()
            .collect();
        for ip in orphans {
            log::info!("ring[{ip}]: camera no longer streamable — stopping its ring");
            self.stop_ring(&ip);
        }
    }

    // ---- recording ----------------------------------------------------------

    pub fn start(
        &self,
        camera_ip: &str,
        requested_pre_roll_secs: u32,
    ) -> Result<(String, bool), String> {
        {
            let sessions = self.sessions.lock().unwrap();
            if let Some((id, _)) = sessions.iter().find(|(_, s)| s.camera_ip == camera_ip) {
                return Ok((id.clone(), false));
            }
        }

        self.start_ring(camera_ip);

        let mut sessions = self.sessions.lock().unwrap();
        if let Some((id, _)) = sessions.iter().find(|(_, s)| s.camera_ip == camera_ip) {
            return Ok((id.clone(), false));
        }

        let ts = chrono::Utc::now();
        let clip_id = format!("{}-{}", camera_ip.replace('.', "_"), ts.timestamp());
        let started_at = ts.to_rfc3339();
        let output = self.dir.join(format!("{clip_id}.mp4"));
        let max_pre = requested_pre_roll_secs.min(self.config.pre_roll_secs);

        // --- Step 1: start the muxer (ffmpeg, or linked GStreamer on a gstreamer-linked box —
        // card #138 A2) -----------------------------------------------------
        let (muxer, mut mux_sink) = start_muxer(
            &output,
            crate::media::Backend::resolve(&self.config.media_backend),
        )?;

        // --- Step 2: snapshot pre-roll + install live sink ATOMICALLY -------
        // Both happen under the one mem_ring lock so the next access unit the
        // collector completes goes to the live channel (not the snapshot) with
        // no gap or overlap at the boundary.
        // The pre-roll window is a DURATION, not a frame count — so a ring holding frames from
        // before a disconnect can never leak into this clip (#101). Frames older than `max_pre`
        // were already evicted by the collector; this just picks the decodable start inside it.
        let window = Duration::from_secs(max_pre.max(1) as u64);
        // Pre-roll as individual frames (bytes + capture time), not one flattened blob: the mp4
        // muxer assigns a PTS per frame (card #138 A2). The ffmpeg path writes them back-to-back,
        // which is byte-identical to the old concatenation.
        type PreRoll = Vec<(Vec<u8>, std::time::Instant)>;
        let (pre_roll, live_rx): (
            PreRoll,
            std::sync::mpsc::Receiver<(Vec<u8>, std::time::Instant)>,
        ) = {
            let rings = self.ring_sessions.lock().unwrap();
            let (live_tx, live_rx) =
                std::sync::mpsc::sync_channel::<(Vec<u8>, std::time::Instant)>(256);
            let frames: PreRoll = match rings.get(camera_ip) {
                None => vec![],
                Some(rs) => {
                    let mut ring = rs.mem_ring.lock().unwrap();
                    let now = std::time::Instant::now();
                    // First frame within the last `window` of wall-clock time.
                    let win_start = ring
                        .frames
                        .iter()
                        .position(|f| now.duration_since(f.at) <= window)
                        .unwrap_or(ring.frames.len());
                    // Earliest keyframe inside the window → most pre-roll. If the GOP is longer than
                    // the window (no keyframe inside it), fall back to the last keyframe overall so
                    // the clip still starts decodable — but never earlier than one GOP before the
                    // window, which the age-eviction already bounds.
                    let start_idx = ring
                        .frames
                        .iter()
                        .enumerate()
                        .skip(win_start)
                        .find(|(_, f)| f.is_keyframe)
                        .map(|(i, _)| i)
                        .or_else(|| ring.frames.iter().rposition(|f| f.is_keyframe));
                    let frames: PreRoll = match start_idx {
                        None => {
                            log::warn!(
                                "recording[{clip_id}]: no keyframe in ring yet, no pre-roll"
                            );
                            vec![]
                        }
                        Some(from) => {
                            let mut b: PreRoll = Vec::new();
                            for f in ring.frames.range(from..) {
                                b.push((f.data.clone(), f.at));
                            }
                            log::info!(
                                "recording[{clip_id}]: pre-roll {} frames from memory ring",
                                ring.frames.len() - from
                            );
                            b
                        }
                    };
                    ring.live_sink = Some(live_tx);
                    frames
                }
            };
            (frames, live_rx)
        };

        // --- Step 3: write pre-roll, then hand the sink to the relay thread --
        // Frame-by-frame (not one blob) so each carries its capture time; for ffmpeg the bytes land
        // back-to-back exactly as the old concatenation did.
        let span: Arc<Mutex<Option<(std::time::Instant, std::time::Instant)>>> =
            Arc::new(Mutex::new(None));
        for (data, at) in &pre_roll {
            mux_sink
                .write_frame(data, *at)
                .map_err(|e| format!("pre-roll write: {e}"))?;
            note_span(&span, *at);
        }

        // With pre-roll present, live frames are contiguous with it (same GOP),
        // so write them immediately. Without pre-roll, wait for the first live
        // IDR so the file starts on a keyframe.
        let wait_for_idr = pre_roll.is_empty();
        let span_relay = Arc::clone(&span);
        let relay = std::thread::spawn(move || {
            let mut idr_seen = !wait_for_idr;
            for (frame_data, at) in live_rx {
                if !idr_seen {
                    let is_idr = frame_data.windows(5).any(|w| {
                        (w[0..4] == [0, 0, 0, 1] && (w[4] & 0x1F) == 5)
                            || (w[0..3] == [0, 0, 1] && (w[3] & 0x1F) == 5)
                    });
                    if is_idr {
                        idr_seen = true;
                    } else {
                        continue;
                    }
                }
                if mux_sink.write_frame(&frame_data, at).is_err() {
                    break;
                }
                note_span(&span_relay, at);
            }
            // EOF (ffmpeg stdin drop) / EOS (appsrc) → the muxer finalises the file.
            mux_sink.finish();
            // stdin drops here → the muxer gets EOF → finishes the MP4
        });

        log::info!("recording started: {clip_id}");
        sessions.insert(
            clip_id.clone(),
            Session {
                clip_id: clip_id.clone(),
                camera_ip: camera_ip.to_string(),
                output,
                started_at,
                relay_thread: Some(relay),
                muxer,
                span,
            },
        );
        Ok((clip_id, true))
    }

    pub fn stop(&self, clip_id: &str) -> Result<ClipMeta, String> {
        let mut sess = {
            let mut sessions = self.sessions.lock().unwrap();
            sessions
                .remove(clip_id)
                .ok_or_else(|| format!("no active session for {clip_id}"))?
        };

        // Clearing live_sink → channel closes → relay thread drains → stdin drops → ffmpeg EOF
        {
            let rings = self.ring_sessions.lock().unwrap();
            if let Some(rs) = rings.get(&sess.camera_ip) {
                rs.mem_ring.lock().unwrap().live_sink = None;
            }
        }

        // Wait for relay (it exits when live_rx drains and stdin closes)
        if let Some(t) = sess.relay_thread.take() {
            t.join().ok();
        }

        // Wait for the muxer to write the final moov atom
        let _ = sess.muxer.wait();

        let size_bytes = fs::metadata(&sess.output).map(|m| m.len()).unwrap_or(0) as i64;
        if size_bytes < 1000 {
            log::warn!(
                "recording[{clip_id}]: clip is only {size_bytes}B — ring may not have been warm"
            );
        }

        let started = chrono::DateTime::parse_from_rfc3339(&sess.started_at)
            .map(|t| t.with_timezone(&chrono::Utc))
            .unwrap_or_else(|_| chrono::Utc::now());
        // Duration from the FOOTAGE the clip actually contains (card #138 A3): the span between the
        // first and last frame we wrote. That needs no `ffprobe` subprocess, and it is truer than
        // both fallbacks — wall-clock counts the wait for the first IDR and any camera dropout as
        // footage, and re-reading the file only tells us what the muxer already knew.
        // `ffprobe` remains a last resort for a clip whose span we somehow never recorded.
        let span = *sess.span.lock().unwrap();
        let duration_secs = span
            .map(|(first, last)| last.saturating_duration_since(first).as_secs() as i32)
            .or_else(|| probe_duration(&sess.output))
            .unwrap_or_else(|| (chrono::Utc::now() - started).num_seconds().max(0) as i32);
        let friendly_name = generate_friendly_name(started.timestamp());

        let sidecar = serde_json::json!({
            "clip_id":       &sess.clip_id,
            "friendly_name": &friendly_name,
            "camera_ip":     &sess.camera_ip,
            "duration_secs": duration_secs,
            "size_bytes":    size_bytes,
            "started_at":    &sess.started_at,
        });
        if let Err(e) = fs::write(
            self.dir.join(format!("{clip_id}.json")),
            sidecar.to_string(),
        ) {
            log::warn!("recording[{clip_id}]: sidecar write: {e}");
        }

        log::info!("recording stopped: {clip_id} ({duration_secs}s, {size_bytes}B)");
        Ok(ClipMeta {
            clip_id: sess.clip_id,
            friendly_name,
            camera_ip: sess.camera_ip,
            duration_secs,
            size_bytes,
            started_at: sess.started_at,
        })
    }

    pub fn stop_by_ip(&self, camera_ip: &str) -> Option<Result<ClipMeta, String>> {
        let clip_id = self
            .sessions
            .lock()
            .unwrap()
            .iter()
            .find(|(_, s)| s.camera_ip == camera_ip)
            .map(|(id, _)| id.clone());
        clip_id.map(|id| self.stop(&id))
    }

    pub fn clip_path(&self, clip_id: &str) -> Option<PathBuf> {
        let p = self.dir.join(format!("{clip_id}.mp4"));
        p.exists().then_some(p)
    }

    pub fn list_clips(&self, p: &ListParams) -> ClipPage {
        let mut items: Vec<ClipListItem> = fs::read_dir(&self.dir)
            .into_iter()
            .flatten()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().map(|x| x == "json").unwrap_or(false))
            .filter_map(|e| {
                let raw = fs::read_to_string(e.path()).ok()?;
                serde_json::from_str::<ClipListItem>(&raw).ok()
            })
            .filter(|c| {
                if let Some(ref ip) = p.camera_ip {
                    if &c.camera_ip != ip {
                        return false;
                    }
                }
                if let Some(ref from) = p.from {
                    if c.started_at.as_str() < from.as_str() {
                        return false;
                    }
                }
                if let Some(ref to) = p.to {
                    if c.started_at.as_str() > to.as_str() {
                        return false;
                    }
                }
                true
            })
            .collect();

        let desc = p.dir == "desc";
        match p.sort.as_str() {
            "duration_secs" => items.sort_by(|a, b| a.duration_secs.cmp(&b.duration_secs)),
            "size_bytes" => items.sort_by(|a, b| a.size_bytes.cmp(&b.size_bytes)),
            "friendly_name" => items.sort_by(|a, b| a.friendly_name.cmp(&b.friendly_name)),
            _ => items.sort_by(|a, b| a.started_at.cmp(&b.started_at)),
        }
        if desc {
            items.reverse();
        }
        let total = items.len();
        ClipPage {
            total,
            items: items.into_iter().skip(p.offset).take(p.limit).collect(),
        }
    }

    pub fn storage_stats(&self) -> StorageStats {
        StorageStats::compute(&self.dir, self.config.max_storage_mb)
    }

    pub fn evict_if_over_limit(&self) -> usize {
        let max_bytes = self.config.max_storage_mb.saturating_mul(1024 * 1024);
        if max_bytes == 0 {
            return 0;
        }
        let target = (max_bytes as f64 * 0.80) as u64;

        let mut clips: Vec<(String, u64, String)> = fs::read_dir(&self.dir)
            .into_iter()
            .flatten()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().map(|x| x == "json").unwrap_or(false))
            .filter_map(|e| {
                let raw = fs::read_to_string(e.path()).ok()?;
                let v = serde_json::from_str::<serde_json::Value>(&raw).ok()?;
                Some((
                    v.get("clip_id")?.as_str()?.to_string(),
                    v.get("size_bytes").and_then(|b| b.as_u64()).unwrap_or(0),
                    v.get("started_at")
                        .and_then(|s| s.as_str())
                        .unwrap_or("")
                        .to_string(),
                ))
            })
            .collect();
        clips.sort_by(|a, b| a.2.cmp(&b.2));

        let total: u64 = clips.iter().map(|(_, sz, _)| sz).sum();
        if total <= max_bytes {
            return 0;
        }

        let active = self
            .sessions
            .lock()
            .unwrap()
            .values()
            .map(|s| s.clip_id.clone())
            .collect::<std::collections::HashSet<_>>();

        let mut remaining = total;
        let mut deleted = 0;
        for (id, sz, _) in &clips {
            if remaining <= target {
                break;
            }
            if active.contains(id) {
                continue;
            }
            if self.delete_clip(id).is_ok() {
                remaining = remaining.saturating_sub(*sz);
                deleted += 1;
                log::info!("storage evict: deleted {id} ({sz}B)");
            }
        }
        deleted
    }

    pub fn delete_clip(&self, clip_id: &str) -> Result<(), String> {
        let mp4 = self.dir.join(format!("{clip_id}.mp4"));
        let json = self.dir.join(format!("{clip_id}.json"));
        if !mp4.exists() {
            return Err(format!("clip not found: {clip_id}"));
        }
        fs::remove_file(&mp4).map_err(|e| format!("remove mp4: {e}"))?;
        let _ = fs::remove_file(&json);
        Ok(())
    }
}

// ---- RTSP ring collector (raw RTP) ------------------------------------------

/// Async task: opens an RTSP connection to go2rtc's local re-server for `camera_ip`,
/// reassembles raw RTP into H264 Annex-B access units, and maintains the in-memory
/// ring. Reconnects automatically on error. Exits when the stop signal fires.
async fn run_rtsp_collector(
    camera_ip: String,
    dial: Option<String>,
    pre_roll_secs: u32,
    mem_ring: Arc<Mutex<MemRing>>,
    stop_rx: std::sync::mpsc::Receiver<()>,
) {
    // The camera itself when we can dial it, go2rtc's re-serve only as the fallback for a camera
    // whose credentials never resolved — the last consumer to leave the sidecar.
    let rtsp_url = dial.unwrap_or_else(|| {
        let ip_key = camera_ip.replace('.', "_");
        format!("rtsp://127.0.0.1:8554/cam_{ip_key}")
    });
    // Keep ~pre_roll_secs * 30fps + 10% headroom of frames in the ring.
    let max_frames = (pre_roll_secs as usize) * 33;

    log::info!("ring-rtsp[{camera_ip}]: collector started → {rtsp_url}");

    loop {
        if stop_rx.try_recv().is_ok() {
            log::info!("ring-rtsp[{camera_ip}]: collector stopped");
            break;
        }

        let result = rtsp_stream_once(
            &rtsp_url,
            &camera_ip,
            max_frames,
            pre_roll_secs,
            &mem_ring,
            &stop_rx,
        )
        .await;

        match result {
            Ok(()) => break, // stop signal received inside
            Err(e) => {
                log::warn!("ring-rtsp[{camera_ip}]: {e} — reconnecting in 3s");
                tokio::time::sleep(Duration::from_secs(3)).await;
            }
        }
    }
}

/// Split `rtsp://user:pass@host/path` into the URL `retina` will accept and the credentials it wants
/// separately. Anything without a userinfo section comes back untouched. Reused by the Tegra capture
/// path (`capture::retina_feed`), which dials the camera with retina for the same reason recording
/// does — GStreamer's `rtspsrc` can't.
pub(crate) fn split_rtsp_credentials(url: &str) -> (String, Option<(String, String)>) {
    let Some((scheme, rest)) = url.split_once("://") else {
        return (url.to_string(), None);
    };
    // Only the userinfo section may hold an `@`; split at the LAST one so a password containing `@`
    // does not truncate the host.
    let Some((userinfo, host)) = rest.rsplit_once('@') else {
        return (url.to_string(), None);
    };
    let (user, pass) = userinfo.split_once(':').unwrap_or((userinfo, ""));
    (
        format!("{scheme}://{host}"),
        Some((user.to_string(), pass.to_string())),
    )
}

async fn rtsp_stream_once(
    rtsp_url: &str,
    camera_ip: &str,
    max_frames: usize,
    pre_roll_secs: u32,
    mem_ring: &Arc<Mutex<MemRing>>,
    stop_rx: &std::sync::mpsc::Receiver<()>,
) -> Result<(), String> {
    // A gap larger than this between consecutive frames is a discontinuity (reconnect, freeze),
    // not a slow stream: at 30fps frames are ~33ms apart, so 2s is ~60 missing frames. On a gap the
    // ring is stale by definition and gets cleared, so a clip started right after a reconnect never
    // inherits pre-gap footage.
    const GAP_RESET: Duration = Duration::from_secs(2);
    let window = Duration::from_secs(pre_roll_secs.max(1) as u64);
    // `retina` REFUSES a URL with inline credentials ("URL must not contain credentials") — they
    // have to be handed over as session options instead. go2rtc's re-serve needed none, so this only
    // showed up once the ring started dialling the camera directly.
    let (bare, creds) = split_rtsp_credentials(rtsp_url);
    let url = bare
        .parse::<url::Url>()
        .map_err(|e| format!("bad URL: {e}"))?;
    let opts = match creds {
        Some((username, password)) => retina::client::SessionOptions::default()
            .creds(Some(retina::client::Credentials { username, password })),
        None => retina::client::SessionOptions::default(),
    };

    let mut session = retina::client::Session::describe(url, opts)
        .await
        .map_err(|e| format!("describe: {e}"))?;

    // Set up the first H264 video track.
    let video_idx = session
        .streams()
        .iter()
        .position(|s| s.media() == "video" && s.encoding_name().eq_ignore_ascii_case("h264"))
        .ok_or_else(|| "no H264 stream in go2rtc RTSP output".to_string())?;

    session
        .setup(video_idx, retina::client::SetupOptions::default())
        .await
        .map_err(|e| format!("setup: {e}"))?;

    // Raw RTP stream — NO .demuxed(): retina's codec layer rejects go2rtc's
    // repackaged NALs. We depacketize ourselves below.
    let mut playing = session
        .play(retina::client::PlayOptions::default())
        .await
        .map_err(|e| format!("play: {e}"))?;

    log::debug!("ring-rtsp[{camera_ip}]: RTSP playing (raw RTP)");

    let mut reasm = H264Reassembler::default();

    loop {
        if stop_rx.try_recv().is_ok() {
            return Ok(()); // clean stop
        }

        // Poll next packet with a timeout so we can check the stop signal.
        let item = tokio::time::timeout(Duration::from_secs(5), playing.next()).await;

        let pkt = match item {
            Err(_) => return Err("RTSP read timeout".to_string()),
            Ok(None) => return Err("RTSP stream ended".to_string()),
            Ok(Some(Err(e))) => return Err(format!("packet error: {e}")),
            Ok(Some(Ok(PacketItem::Rtp(p)))) => p,
            Ok(Some(Ok(_))) => continue, // RTCP etc.
        };

        let ts = pkt.timestamp().timestamp();
        let marker = pkt.mark();
        for frame in reasm.push(pkt.payload(), ts, marker) {
            if frame.data.is_empty() {
                continue;
            }
            let mut ring = mem_ring.lock().unwrap();
            // Discontinuity? Everything already in the ring predates a gap — drop it, so pre-roll
            // can never splice pre-gap frames onto what comes next (#101).
            if let Some(last) = ring.frames.back() {
                if frame.at.duration_since(last.at) > GAP_RESET {
                    log::info!(
                        "ring-rtsp[{camera_ip}]: {:?} gap — clearing stale pre-roll",
                        frame.at.duration_since(last.at)
                    );
                    ring.frames.clear();
                }
            }
            // Forward to an active recording first (clone), then store in ring.
            if let Some(tx) = ring.live_sink.as_ref() {
                let _ = tx.try_send((frame.data.clone(), frame.at)); // non-blocking; drop on full
            }
            let now = frame.at;
            ring.frames.push_back(frame);
            // Evict by AGE — the ring holds only the last `window` of real footage. A count cap
            // stays as a hard memory backstop (a high-fps camera can't balloon it).
            while ring
                .frames
                .front()
                .map(|f| now.duration_since(f.at) > window)
                .unwrap_or(false)
            {
                ring.frames.pop_front();
            }
            while ring.frames.len() > max_frames {
                ring.frames.pop_front();
            }
        }
    }
}

// ---- H264 RTP depacketizer (RFC 6184, lenient) ------------------------------

/// Reassembles RTP payloads into H264 Annex-B *access units* (one per frame).
/// Handles single-NAL, STAP-A aggregation, and FU-A fragmentation. Caches the
/// most recent SPS/PPS and prepends them to keyframe access units that lack
/// inline parameter sets, so every keyframe in the ring is independently
/// decodable. Deliberately lenient — unlike retina's codec layer, it does not
/// reject NALs for spec-purity reasons; ffmpeg copes fine.
#[derive(Default)]
struct H264Reassembler {
    fu_buf: Vec<u8>, // in-progress FU-A NAL (incl. reconstructed header)
    fu_active: bool,
    au: Vec<u8>, // accumulating access unit (Annex-B)
    au_ts: Option<i64>,
    au_has_idr: bool,
    au_has_sps: bool,
    sps: Option<Vec<u8>>, // raw NAL (no start code)
    pps: Option<Vec<u8>>,
}

const START_CODE: [u8; 4] = [0, 0, 0, 1];

impl H264Reassembler {
    /// Feed one RTP payload. Returns any completed access units.
    fn push(&mut self, payload: &[u8], ts: i64, marker: bool) -> Vec<H264Frame> {
        let mut out = Vec::new();
        if payload.is_empty() {
            return out;
        }

        // Timestamp change ⇒ previous access unit is complete.
        if let Some(cur) = self.au_ts {
            if cur != ts {
                if let Some(f) = self.flush_au() {
                    out.push(f);
                }
            }
        }
        self.au_ts = Some(ts);

        let nal_type = payload[0] & 0x1F;
        match nal_type {
            1..=23 => self.add_nal(payload), // single NAL unit packet
            24 => {
                // STAP-A: [hdr][size:u16][nal]...[size:u16][nal]
                let mut i = 1;
                while i + 2 <= payload.len() {
                    let size = u16::from_be_bytes([payload[i], payload[i + 1]]) as usize;
                    i += 2;
                    if i + size > payload.len() {
                        break;
                    }
                    self.add_nal(&payload[i..i + size]);
                    i += size;
                }
            }
            28 => {
                // FU-A: [FU indicator][FU header][fragment]
                if payload.len() < 3 {
                    return out;
                }
                let fu_ind = payload[0];
                let fu_hdr = payload[1];
                let start = fu_hdr & 0x80 != 0;
                let end = fu_hdr & 0x40 != 0;
                let orig = fu_hdr & 0x1F;
                if start {
                    self.fu_buf.clear();
                    self.fu_buf.push((fu_ind & 0xE0) | orig); // reconstructed NAL header
                    self.fu_active = true;
                }
                if self.fu_active {
                    self.fu_buf.extend_from_slice(&payload[2..]);
                    if end {
                        let nal = std::mem::take(&mut self.fu_buf);
                        self.fu_active = false;
                        self.add_nal(&nal);
                    }
                }
            }
            _ => { /* STAP-B / MTAP / FU-B / reserved — skip */ }
        }

        if marker {
            if let Some(f) = self.flush_au() {
                out.push(f);
            }
        }
        out
    }

    fn add_nal(&mut self, nal: &[u8]) {
        if nal.is_empty() {
            return;
        }
        match nal[0] & 0x1F {
            7 => {
                self.sps = Some(nal.to_vec());
                self.au_has_sps = true;
            }
            8 => {
                self.pps = Some(nal.to_vec());
            }
            5 => {
                self.au_has_idr = true;
            }
            _ => {}
        }
        self.au.extend_from_slice(&START_CODE);
        self.au.extend_from_slice(nal);
    }

    fn flush_au(&mut self) -> Option<H264Frame> {
        if self.au.is_empty() {
            self.reset_au();
            return None;
        }
        let is_keyframe = self.au_has_idr;
        let mut data = Vec::with_capacity(self.au.len() + 64);
        // Prepend cached SPS/PPS to keyframes that lack inline parameter sets.
        if self.au_has_idr && !self.au_has_sps {
            if let (Some(sps), Some(pps)) = (&self.sps, &self.pps) {
                data.extend_from_slice(&START_CODE);
                data.extend_from_slice(sps);
                data.extend_from_slice(&START_CODE);
                data.extend_from_slice(pps);
            }
        }
        data.append(&mut self.au);
        self.reset_au();
        Some(H264Frame {
            data,
            is_keyframe,
            at: std::time::Instant::now(),
        })
    }

    fn reset_au(&mut self) {
        self.au.clear();
        self.au_ts = None;
        self.au_has_idr = false;
        self.au_has_sps = false;
    }
}

// ---- helpers ----------------------------------------------------------------

/// Clip duration probe (A3). ffmpeg-only — see [`spawn_muxer`] on why card #138's GStreamer path
/// doesn't apply here (there's no GStreamer-muxed clip to probe).
fn probe_duration(path: &PathBuf) -> Option<i32> {
    let out = crate::proc::command("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "default=noprint_wrappers=1:nokey=1",
            path.to_str().unwrap_or_default(),
        ])
        .output()
        .ok()?;
    std::str::from_utf8(&out.stdout)
        .ok()?
        .trim()
        .parse::<f64>()
        .ok()
        .map(|d| d.round() as i32)
}

// ---- metadata ---------------------------------------------------------------

pub struct ClipMeta {
    pub clip_id: String,
    pub friendly_name: String,
    pub camera_ip: String,
    pub duration_secs: i32,
    pub size_bytes: i64,
    pub started_at: String,
}

// ---- config fetch -----------------------------------------------------------

fn fetch_config(control_url: &str) -> Option<RecordingConfig> {
    let url = format!("{control_url}/config/recording");
    let mut req = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(10))
        .build()
        .get(&url);
    if let Ok(t) = std::env::var("BOX_TOKEN") {
        let t = t.trim();
        if !t.is_empty() {
            req = req.set("Authorization", &format!("Bearer {t}"));
        }
    }
    crate::signer::sign(req, "GET", &url, b"")
        .call()
        .ok()?
        .into_json::<RecordingConfig>()
        .ok()
}

#[cfg(test)]
mod rtsp_credentials_tests {
    use super::split_rtsp_credentials;

    /// `retina` rejects a URL with inline credentials outright, so this split is what makes the ring
    /// able to dial a camera at all.
    #[test]
    fn splits_userinfo_out_of_the_url() {
        let (url, creds) = split_rtsp_credentials("rtsp://bob:s3cret@10.0.0.2:554/stream1");
        assert_eq!(url, "rtsp://10.0.0.2:554/stream1");
        assert_eq!(creds, Some(("bob".into(), "s3cret".into())));
    }

    /// go2rtc's re-serve carries no credentials — that path must stay byte-identical.
    #[test]
    fn leaves_a_credential_less_url_untouched() {
        let (url, creds) = split_rtsp_credentials("rtsp://127.0.0.1:8554/cam_10_0_0_2");
        assert_eq!(url, "rtsp://127.0.0.1:8554/cam_10_0_0_2");
        assert_eq!(creds, None);
    }

    /// A password containing `@` is legal and common; splitting at the first one would truncate the
    /// host and dial nowhere.
    #[test]
    fn splits_at_the_last_at_so_an_at_in_the_password_survives() {
        let (url, creds) = split_rtsp_credentials("rtsp://bob:p@ss@10.0.0.2:554/s");
        assert_eq!(url, "rtsp://10.0.0.2:554/s");
        assert_eq!(creds, Some(("bob".into(), "p@ss".into())));
    }
}
