//! On-demand clip recording with an in-memory H264 pre-roll ring buffer.
//!
//! **Ring buffer** (`start_ring`)
//!   A background thread per camera opens an RTSP connection to go2rtc's local
//!   re-server (`rtsp://127.0.0.1:8554/cam_{ip_key}`) via the `retina` crate.
//!   Received H264 Annex-B frames are kept in a `VecDeque<H264Frame>` in memory
//!   (the *mem-ring*). No segments are written to disk between recordings.
//!   go2rtc owns the camera RTSP connection; retina is just another consumer.
//!
//! **Recording** (`start` / `stop`)
//!   `start()` snapshots the mem-ring from the last IDR frame (guarantees a clean
//!   keyframe — no black frames), spawns `ffmpeg -f h264 -i pipe:0 … output.mp4`,
//!   writes the pre-roll bytes to ffmpeg stdin, then wires the live_sink channel so
//!   the collector thread keeps feeding new frames to ffmpeg for the live portion.
//!
//!   `stop()` drops the live_sink → channel closes → relay thread exits → ffmpeg
//!   stdin gets EOF → ffmpeg finalises the MP4. No concat step needed.

use futures::StreamExt as _;
use retina::codec::CodecItem;
use std::collections::{HashMap, VecDeque};
use std::fs;
use std::io::Write as _;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

// ---- friendly name word list ------------------------------------------------

const ADJECTIVES: &[&str] = &[
    "amber","azure","bold","bright","calm","cedar","clear","cloud","coral","crisp",
    "dark","dawn","deep","deft","dense","drift","dusk","early","faint","fast",
    "fern","firm","flash","fleet","frost","gentle","ghost","glad","glide","gold",
    "grand","grey","gust","hardy","haze","high","hollow","iron","jade","keen",
    "kind","lake","lean","light","lofty","lone","loud","mist","misty","moss",
    "muted","night","noble","north","oak","pale","peak","pine","plain","prime",
    "pure","quick","quiet","rapid","raw","reef","rich","ridge","ripple","rise",
    "roam","rocky","rough","sage","sharp","silver","sleek","slim","slow","smooth",
    "soft","solid","stark","steady","steel","still","stone","storm","strong","swift",
    "tall","thin","tidal","trim","true","vast","veil","warm","wide","wild",
];

const NOUNS: &[&str] = &[
    "arc","ash","bay","beam","birch","blade","blaze","bloom","boulder","branch",
    "bridge","brook","canyon","cedar","cliff","cloud","comet","cove","creek","crest",
    "dawn","delta","drift","dune","dusk","echo","ember","falcon","fern","field",
    "fjord","flame","flare","flash","flint","fog","forest","gale","gate","glacier",
    "glen","grove","gust","harbor","hawk","haze","hill","hollow","horizon","isle",
    "jade","lake","ledge","light","loch","mesa","meteor","mist","moon","moss",
    "mount","nebula","night","north","oak","orbit","pass","peak","pine","plain",
    "pond","pulse","quarry","rain","range","rapid","reef","ridge","rift","river",
    "rock","root","run","sage","salt","sand","shore","sierra","sky","slate",
    "snow","sol","spark","spire","spring","star","stone","storm","stream","summit",
    "sun","surge","tide","timber","torch","trail","vale","vault","wave","wind",
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
    pub clip_id:       String,
    pub friendly_name: String,
    pub camera_ip:     String,
    pub duration_secs: i32,
    pub size_bytes:    i64,
    pub started_at:    String,
}

#[derive(Debug, serde::Serialize)]
pub struct ClipPage {
    pub total: usize,
    pub items: Vec<ClipListItem>,
}

#[derive(Debug)]
pub struct ListParams {
    pub camera_ip: Option<String>,
    pub sort:      String,
    pub dir:       String,
    pub limit:     usize,
    pub offset:    usize,
    pub from:      Option<String>,
    pub to:        Option<String>,
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
    #[serde(default = "default_motion_var")]
    pub motion_var_threshold: f64,
    #[serde(default = "default_recordings_path")]
    pub recordings_path: String,
}

impl Default for RecordingConfig {
    fn default() -> Self {
        Self {
            pre_roll_secs: 10,
            max_storage_mb: 1000,
            codec: "copy".to_string(),
            cameras_always_warm: false,
            motion_var_threshold: 16.0,
            recordings_path: default_recordings_path(),
        }
    }
}

fn default_motion_var() -> f64 { 16.0 }
fn default_pre_roll() -> u32 { 10 }
fn default_storage() -> u64 { 1000 }
fn default_codec() -> String { "copy".to_string() }
fn default_recordings_path() -> String { "/opt/sentinel-edge/recordings".to_string() }

// ---- storage stats ----------------------------------------------------------

#[derive(Debug, Clone, serde::Serialize)]
pub struct StorageStats {
    pub used_bytes: u64,
    pub max_bytes:  u64,
    pub clip_count: usize,
    pub path:       String,
    pub pct:        u8,
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
        let pct = if max_bytes == 0 { 100 } else { ((used_bytes * 100) / max_bytes).min(100) as u8 };
        StorageStats { used_bytes, max_bytes, clip_count, path: dir.to_string_lossy().into_owned(), pct }
    }
}

// ---- in-memory ring buffer --------------------------------------------------

struct H264Frame {
    data:        Vec<u8>, // H264 Annex-B bytes
    is_keyframe: bool,
}

struct MemRing {
    frames: VecDeque<H264Frame>,
}

// ---- ring session -----------------------------------------------------------

/// Per-camera ring state. The collector thread owns the retina RTSP connection
/// and writes frames into `mem_ring`. During a recording, it also sends frames
/// to `live_sink` so they reach the active ffmpeg process.
struct RingSession {
    #[allow(dead_code)]
    stop_tx:   std::sync::mpsc::SyncSender<()>,
    mem_ring:  Arc<Mutex<MemRing>>,
    /// Set to Some(tx) by start(), cleared to None by stop().
    live_sink: Arc<Mutex<Option<std::sync::mpsc::SyncSender<Vec<u8>>>>>,
}

// ---- active recording -------------------------------------------------------

struct Session {
    clip_id:      String,
    camera_ip:    String,
    output:       PathBuf,
    started_at:   String,
    relay_thread: Option<std::thread::JoinHandle<()>>,
    ffmpeg:       Child,
}

// ---- manager ----------------------------------------------------------------

pub struct RecordingManager {
    pub config:    RecordingConfig,
    dir:           PathBuf,
    sessions:      Mutex<HashMap<String, Session>>,
    ring_sessions: Arc<Mutex<HashMap<String, RingSession>>>,
}

impl RecordingManager {
    pub fn boot(control_url: &str) -> Arc<RecordingManager> {
        let config = fetch_config(control_url).unwrap_or_default();
        let dir = std::env::var("EDGE_RECORDING_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(&config.recordings_path));
        if let Err(e) = fs::create_dir_all(&dir) {
            log::warn!("recording: mkdir {:?}: {e}", dir);
        }
        log::info!(
            "Recording dir {:?}, pre_roll={}s, always_warm={}",
            dir, config.pre_roll_secs, config.cameras_always_warm
        );
        Arc::new(RecordingManager {
            config,
            dir,
            sessions:      Mutex::new(HashMap::new()),
            ring_sessions: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    // ---- ring ---------------------------------------------------------------

    pub fn start_ring(&self, camera_ip: &str) {
        {
            if self.ring_sessions.lock().unwrap().contains_key(camera_ip) {
                return;
            }
        }

        let (stop_tx, stop_rx) = std::sync::mpsc::sync_channel(1);
        let mem_ring  = Arc::new(Mutex::new(MemRing { frames: VecDeque::new() }));
        let live_sink = Arc::new(Mutex::new(None::<std::sync::mpsc::SyncSender<Vec<u8>>>));

        let ip_t          = camera_ip.to_string();
        let pre_roll_secs = self.config.pre_roll_secs;
        let mem_ring_t    = Arc::clone(&mem_ring);
        let live_sink_t   = Arc::clone(&live_sink);

        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("tokio rt for RTSP ring");
            rt.block_on(run_rtsp_collector(
                ip_t, pre_roll_secs, mem_ring_t, live_sink_t, stop_rx,
            ));
        });

        self.ring_sessions.lock().unwrap().insert(
            camera_ip.to_string(),
            RingSession { stop_tx, mem_ring, live_sink },
        );

        log::info!("ring[{camera_ip}]: started (retina RTSP, {}s in-memory pre-roll)", pre_roll_secs);
    }

    #[allow(dead_code)]
    pub fn stop_ring(&self, camera_ip: &str) {
        if let Some(rs) = self.ring_sessions.lock().unwrap().remove(camera_ip) {
            let _ = rs.stop_tx.try_send(());
        }
    }

    // ---- recording ----------------------------------------------------------

    pub fn start(&self, camera_ip: &str, requested_pre_roll_secs: u32) -> Result<(String, bool), String> {
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

        let ts         = chrono::Utc::now();
        let clip_id    = format!("{}-{}", camera_ip.replace('.', "_"), ts.timestamp());
        let started_at = ts.to_rfc3339();
        let output     = self.dir.join(format!("{clip_id}.mp4"));
        let max_pre    = requested_pre_roll_secs.min(self.config.pre_roll_secs);

        // --- Step 1: snapshot pre-roll from in-memory ring -------------------
        // Estimate max frames to look back: assume sub-stream ≤ 30 fps.
        let max_frames   = (max_pre as usize) * 30;
        let pre_roll_bytes: Vec<u8> = {
            let rings = self.ring_sessions.lock().unwrap();
            match rings.get(camera_ip) {
                None => vec![],
                Some(rs) => {
                    let ring = rs.mem_ring.lock().unwrap();
                    let window_start = ring.frames.len().saturating_sub(max_frames);
                    // Find the LAST keyframe in the window so we always start cleanly.
                    let last_idr = ring.frames.range(window_start..)
                        .rposition(|f| f.is_keyframe)
                        .map(|rel| window_start + rel);
                    match last_idr {
                        None => {
                            log::warn!("recording[{clip_id}]: no keyframe in ring yet, no pre-roll");
                            vec![]
                        }
                        Some(from) => {
                            // Drop the most-recent frame — may still be arriving.
                            let to = ring.frames.len().saturating_sub(1);
                            let mut bytes = Vec::new();
                            for f in ring.frames.range(from..to) {
                                bytes.extend_from_slice(&f.data);
                            }
                            log::info!(
                                "recording[{clip_id}]: pre-roll {} frames from memory ring",
                                to.saturating_sub(from)
                            );
                            bytes
                        }
                    }
                }
            }
        };

        // --- Step 2: spawn ffmpeg, write pre-roll, wire live_sink ------------
        let mut ffmpeg = Command::new("ffmpeg")
            .args([
                "-loglevel", "warning",
                "-f", "h264",
                "-i", "pipe:0",
                "-c:v", "copy",
                "-movflags", "+faststart",
                "-y",
                output.to_str().unwrap_or_default(),
            ])
            .stdin(Stdio::piped())
            .spawn()
            .map_err(|e| format!("ffmpeg spawn: {e}"))?;

        let mut ffmpeg_stdin = ffmpeg.stdin.take().expect("piped");

        if !pre_roll_bytes.is_empty() {
            ffmpeg_stdin.write_all(&pre_roll_bytes)
                .map_err(|e| format!("ffmpeg pre-roll write: {e}"))?;
        }

        // Live frames: collector → channel → relay thread → ffmpeg stdin
        let (live_tx, live_rx) = std::sync::mpsc::sync_channel::<Vec<u8>>(128);
        {
            let rings = self.ring_sessions.lock().unwrap();
            if let Some(rs) = rings.get(camera_ip) {
                *rs.live_sink.lock().unwrap() = Some(live_tx);
            }
        }

        // Relay: buffer until first IDR in live portion so there are no glitches
        // at the pre-roll/live boundary.
        let relay = std::thread::spawn(move || {
            let mut idr_seen = pre_roll_bytes.is_empty(); // if no pre-roll, don't wait for IDR
            for frame_data in live_rx {
                if !idr_seen {
                    // Check for H264 Annex-B IDR NAL (type 5 = 0x65 after start code)
                    let is_idr = frame_data.windows(5).any(|w| {
                        (w[0..4] == [0, 0, 0, 1] && (w[4] & 0x1F) == 5)
                        || (w[0..3] == [0, 0, 1] && (w[3] & 0x1F) == 5)
                    });
                    if is_idr { idr_seen = true; } else { continue; }
                }
                if ffmpeg_stdin.write_all(&frame_data).is_err() {
                    break;
                }
            }
            // stdin drops here → ffmpeg gets EOF → finishes the MP4
        });

        log::info!("recording started: {clip_id}");
        sessions.insert(clip_id.clone(), Session {
            clip_id:      clip_id.clone(),
            camera_ip:    camera_ip.to_string(),
            output,
            started_at,
            relay_thread: Some(relay),
            ffmpeg,
        });
        Ok((clip_id, true))
    }

    pub fn stop(&self, clip_id: &str) -> Result<ClipMeta, String> {
        let mut sess = {
            let mut sessions = self.sessions.lock().unwrap();
            sessions.remove(clip_id)
                .ok_or_else(|| format!("no active session for {clip_id}"))?
        };

        // Dropping live_sink → channel closes → relay thread drains → stdin drops → ffmpeg EOF
        {
            let rings = self.ring_sessions.lock().unwrap();
            if let Some(rs) = rings.get(&sess.camera_ip) {
                *rs.live_sink.lock().unwrap() = None;
            }
        }

        // Wait for relay (it exits when live_rx drains and stdin closes)
        if let Some(t) = sess.relay_thread.take() {
            t.join().ok();
        }

        // Wait for ffmpeg to write the final moov atom
        let _ = sess.ffmpeg.wait();

        let size_bytes = fs::metadata(&sess.output).map(|m| m.len()).unwrap_or(0) as i64;
        if size_bytes < 1000 {
            log::warn!("recording[{clip_id}]: clip is only {size_bytes}B — ring may not have been warm");
        }

        let started = chrono::DateTime::parse_from_rfc3339(&sess.started_at)
            .map(|t| t.with_timezone(&chrono::Utc))
            .unwrap_or_else(|_| chrono::Utc::now());
        let duration_secs = probe_duration(&sess.output)
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
        if let Err(e) = fs::write(self.dir.join(format!("{clip_id}.json")), sidecar.to_string()) {
            log::warn!("recording[{clip_id}]: sidecar write: {e}");
        }

        log::info!("recording stopped: {clip_id} ({duration_secs}s, {size_bytes}B)");
        Ok(ClipMeta {
            clip_id:       sess.clip_id,
            friendly_name,
            camera_ip:     sess.camera_ip,
            duration_secs,
            size_bytes,
            started_at:    sess.started_at,
        })
    }

    pub fn stop_by_ip(&self, camera_ip: &str) -> Option<Result<ClipMeta, String>> {
        let clip_id = self.sessions.lock().unwrap()
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
            .into_iter().flatten().filter_map(|e| e.ok())
            .filter(|e| e.path().extension().map(|x| x == "json").unwrap_or(false))
            .filter_map(|e| {
                let raw = fs::read_to_string(e.path()).ok()?;
                serde_json::from_str::<ClipListItem>(&raw).ok()
            })
            .filter(|c| {
                if let Some(ref ip) = p.camera_ip { if &c.camera_ip != ip { return false; } }
                if let Some(ref from) = p.from { if c.started_at.as_str() < from.as_str() { return false; } }
                if let Some(ref to)   = p.to   { if c.started_at.as_str() > to.as_str()   { return false; } }
                true
            })
            .collect();

        let desc = p.dir == "desc";
        match p.sort.as_str() {
            "duration_secs" => items.sort_by(|a, b| a.duration_secs.cmp(&b.duration_secs)),
            "size_bytes"    => items.sort_by(|a, b| a.size_bytes.cmp(&b.size_bytes)),
            "friendly_name" => items.sort_by(|a, b| a.friendly_name.cmp(&b.friendly_name)),
            _               => items.sort_by(|a, b| a.started_at.cmp(&b.started_at)),
        }
        if desc { items.reverse(); }
        let total = items.len();
        ClipPage { total, items: items.into_iter().skip(p.offset).take(p.limit).collect() }
    }

    pub fn storage_stats(&self) -> StorageStats {
        StorageStats::compute(&self.dir, self.config.max_storage_mb)
    }

    pub fn evict_if_over_limit(&self) -> usize {
        let max_bytes = self.config.max_storage_mb.saturating_mul(1024 * 1024);
        if max_bytes == 0 { return 0; }
        let target = (max_bytes as f64 * 0.80) as u64;

        let mut clips: Vec<(String, u64, String)> = fs::read_dir(&self.dir)
            .into_iter().flatten().filter_map(|e| e.ok())
            .filter(|e| e.path().extension().map(|x| x == "json").unwrap_or(false))
            .filter_map(|e| {
                let raw = fs::read_to_string(e.path()).ok()?;
                let v = serde_json::from_str::<serde_json::Value>(&raw).ok()?;
                Some((
                    v.get("clip_id")?.as_str()?.to_string(),
                    v.get("size_bytes").and_then(|b| b.as_u64()).unwrap_or(0),
                    v.get("started_at").and_then(|s| s.as_str()).unwrap_or("").to_string(),
                ))
            })
            .collect();
        clips.sort_by(|a, b| a.2.cmp(&b.2));

        let total: u64 = clips.iter().map(|(_, sz, _)| sz).sum();
        if total <= max_bytes { return 0; }

        let active = self.sessions.lock().unwrap()
            .values().map(|s| s.clip_id.clone())
            .collect::<std::collections::HashSet<_>>();

        let mut remaining = total;
        let mut deleted = 0;
        for (id, sz, _) in &clips {
            if remaining <= target { break; }
            if active.contains(id) { continue; }
            if self.delete_clip(id).is_ok() {
                remaining = remaining.saturating_sub(*sz);
                deleted += 1;
                log::info!("storage evict: deleted {id} ({sz}B)");
            }
        }
        deleted
    }

    pub fn delete_clip(&self, clip_id: &str) -> Result<(), String> {
        let mp4  = self.dir.join(format!("{clip_id}.mp4"));
        let json = self.dir.join(format!("{clip_id}.json"));
        if !mp4.exists() { return Err(format!("clip not found: {clip_id}")); }
        fs::remove_file(&mp4).map_err(|e| format!("remove mp4: {e}"))?;
        let _ = fs::remove_file(&json);
        Ok(())
    }
}

// ---- retina RTSP ring collector ---------------------------------------------

/// Async task: opens an RTSP connection to go2rtc's local re-server for `camera_ip`,
/// receives H264 Annex-B frames, and maintains the in-memory ring.
/// Reconnects automatically on error. Exits when the stop signal fires.
async fn run_rtsp_collector(
    camera_ip:    String,
    pre_roll_secs: u32,
    mem_ring:      Arc<Mutex<MemRing>>,
    live_sink:     Arc<Mutex<Option<std::sync::mpsc::SyncSender<Vec<u8>>>>>,
    stop_rx:       std::sync::mpsc::Receiver<()>,
) {
    let ip_key      = camera_ip.replace('.', "_");
    let stream_name = format!("cam_{ip_key}");
    let rtsp_url    = format!("rtsp://127.0.0.1:8554/{stream_name}");
    // Keep ~pre_roll_secs * 30fps + 10% headroom of frames in the ring.
    let max_frames  = (pre_roll_secs as usize) * 33;

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
            &mem_ring,
            &live_sink,
            &stop_rx,
        ).await;

        match result {
            Ok(()) => break, // stop signal received inside
            Err(e) => {
                log::warn!("ring-rtsp[{camera_ip}]: {e} — reconnecting in 3s");
                tokio::time::sleep(Duration::from_secs(3)).await;
            }
        }
    }
}

async fn rtsp_stream_once(
    rtsp_url:  &str,
    camera_ip: &str,
    max_frames: usize,
    mem_ring:  &Arc<Mutex<MemRing>>,
    live_sink: &Arc<Mutex<Option<std::sync::mpsc::SyncSender<Vec<u8>>>>>,
    stop_rx:   &std::sync::mpsc::Receiver<()>,
) -> Result<(), String> {
    let url = rtsp_url.parse::<url::Url>().map_err(|e| format!("bad URL: {e}"))?;

    let mut session = retina::client::Session::describe(
        url,
        retina::client::SessionOptions::default(),
    ).await.map_err(|e| format!("describe: {e}"))?;

    // Set up the first H264 video track.
    let video_idx = session.streams().iter().position(|s| {
        s.media() == "video" && s.encoding_name().eq_ignore_ascii_case("h264")
    }).ok_or_else(|| "no H264 stream in go2rtc RTSP output".to_string())?;

    session.setup(video_idx, retina::client::SetupOptions::default()).await
        .map_err(|e| format!("setup: {e}"))?;

    let mut playing = session.play(retina::client::PlayOptions::default()).await
        .map_err(|e| format!("play: {e}"))?
        .demuxed()
        .map_err(|e| format!("demux: {e}"))?;

    log::debug!("ring-rtsp[{camera_ip}]: RTSP playing");

    loop {
        if stop_rx.try_recv().is_ok() {
            return Ok(()); // clean stop
        }

        // Poll next frame with a timeout so we can check the stop signal.
        let item = tokio::time::timeout(Duration::from_secs(5), playing.next()).await;

        let codec_item = match item {
            Err(_) => return Err("RTSP read timeout".to_string()),
            Ok(None) => return Err("RTSP stream ended".to_string()),
            Ok(Some(Err(e))) => return Err(format!("frame error: {e}")),
            Ok(Some(Ok(ci))) => ci,
        };

        let (frame_data, is_keyframe) = match codec_item {
            CodecItem::VideoFrame(f) => {
                let key = f.is_random_access_point();
                let data = f.data().to_vec();
                (data, key)
            }
            _ => continue,
        };

        if frame_data.is_empty() {
            continue;
        }

        // Push to in-memory ring.
        {
            let mut ring = mem_ring.lock().unwrap();
            ring.frames.push_back(H264Frame { data: frame_data.clone(), is_keyframe });
            while ring.frames.len() > max_frames {
                ring.frames.pop_front();
            }
        }

        // Forward to live recording if active.
        {
            let sink = live_sink.lock().unwrap();
            if let Some(ref tx) = *sink {
                let _ = tx.try_send(frame_data); // non-blocking; drop on full
            }
        }
    }
}

// ---- helpers ----------------------------------------------------------------

fn probe_duration(path: &PathBuf) -> Option<i32> {
    let out = Command::new("ffprobe")
        .args([
            "-v", "error",
            "-show_entries", "format=duration",
            "-of", "default=noprint_wrappers=1:nokey=1",
            path.to_str().unwrap_or_default(),
        ])
        .output().ok()?;
    std::str::from_utf8(&out.stdout).ok()?.trim().parse::<f64>().ok().map(|d| d.round() as i32)
}

// ---- metadata ---------------------------------------------------------------

pub struct ClipMeta {
    pub clip_id:       String,
    pub friendly_name: String,
    pub camera_ip:     String,
    pub duration_secs: i32,
    pub size_bytes:    i64,
    pub started_at:    String,
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
    req.call().ok()?.into_json::<RecordingConfig>().ok()
}
