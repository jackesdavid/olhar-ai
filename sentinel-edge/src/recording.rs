//! On-demand clip recording with pre-roll ring buffer.
//!
//! **Ring buffer** (`start_ring` / `stop_ring`)
//!   A per-camera background thread polls go2rtc's HLS endpoint
//!   (`/api/hls/playlist.m3u8?id=…`) every 500 ms, downloads each new MPEG-TS
//!   segment, and saves it to `/tmp/sentinel-ring/{ip}/seg_%06d.ts`. go2rtc
//!   owns the RTSP connection and normalises timestamps — no raw camera RTSP
//!   or ffmpeg process is needed for the ring. A cleanup pass keeps only the
//!   last `pre_roll_secs + 2` segments.
//!
//! **Recording** (`start` / `stop`)
//!   `start(ip, requested_pre_roll_secs)` copies the last N complete ring segments as
//!   the pre-roll, records the wall-clock start time, and sets a "recording lock"
//!   so cleanup never deletes segments with mtime ≥ that time.
//!   `stop(clip_id)` collects ring segments with mtime ≥ start time (live portion),
//!   then runs ffmpeg to concat [preroll copies + live segments] into the final .mp4.
//!
//! **No per-camera ffmpeg processes.** ffmpeg is only invoked once per clip at
//!   stop() time to mux the already-downloaded MPEG-TS segments into MP4.

use std::collections::HashMap;
use std::fs;
use std::io::Read as _;
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

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

/// Generate a memorable 3-word slug from a Unix epoch timestamp.
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
    pub used_bytes:  u64,
    pub max_bytes:   u64,
    pub clip_count:  usize,
    pub path:        String,
    pub pct:         u8,
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
                            let sz = v.get("size_bytes").and_then(|b| b.as_u64()).unwrap_or(0);
                            used_bytes += sz;
                            clip_count += 1;
                        }
                    }
                }
            }
        }
        let max_bytes = max_mb.saturating_mul(1024 * 1024);
        let pct = if max_bytes == 0 { 100 } else {
            ((used_bytes * 100) / max_bytes).min(100) as u8
        };
        StorageStats { used_bytes, max_bytes, clip_count, path: dir.to_string_lossy().into_owned(), pct }
    }
}

// ---- ring session -----------------------------------------------------------

/// A live ring-buffer collector for one camera. Sending on `stop_tx` causes the
/// background thread to drain its current segment download and exit within one poll
/// interval (≤ 500 ms).
struct RingSession {
    #[allow(dead_code)] // used via try_send() after consuming from HashMap in stop_ring()
    stop_tx: std::sync::mpsc::SyncSender<()>,
}

// ---- live recording session -------------------------------------------------

struct Session {
    clip_id:         String,
    camera_ip:       String,
    output:          PathBuf,
    /// Pre-roll segments copied into live_dir at start() time.
    preroll:         Vec<PathBuf>,
    /// Wall-clock time when recording started. Ring segments with mtime ≥ this
    /// value are the live portion of the clip; cleanup never touches them.
    ring_started_at: SystemTime,
    started_at:      String,
}

// ---- manager ----------------------------------------------------------------

pub struct RecordingManager {
    pub config: RecordingConfig,
    dir:          PathBuf,
    ring_dir:     PathBuf,
    live_dir:     PathBuf,
    sessions:        Mutex<HashMap<String, Session>>,
    /// Arc so the per-camera collector thread can hold a weak reference without Self.
    ring_sessions:   Arc<Mutex<HashMap<String, RingSession>>>,
    /// Per-camera recording start time. Cleanup skips segments newer than this.
    recording_locks: Arc<Mutex<HashMap<String, SystemTime>>>,
}

impl RecordingManager {
    pub fn boot(control_url: &str) -> Arc<RecordingManager> {
        let config = fetch_config(control_url).unwrap_or_default();
        let dir = std::env::var("EDGE_RECORDING_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(&config.recordings_path));
        let ring_dir = PathBuf::from("/tmp/sentinel-ring");
        let live_dir = PathBuf::from("/tmp/sentinel-live");

        for d in [&dir, &ring_dir, &live_dir] {
            if let Err(e) = fs::create_dir_all(d) {
                log::warn!("recording: mkdir {:?}: {e}", d);
            }
        }
        log::info!(
            "Recording dir {:?}, pre_roll_max={}s, always_warm={}",
            dir, config.pre_roll_secs, config.cameras_always_warm
        );
        Arc::new(RecordingManager {
            config,
            dir,
            ring_dir,
            live_dir,
            sessions:        Mutex::new(HashMap::new()),
            ring_sessions:   Arc::new(Mutex::new(HashMap::new())),
            recording_locks: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    // ---- ring ---------------------------------------------------------------

    /// Start the go2rtc HLS ring collector for `camera_ip`. Idempotent — no-op
    /// if already running. The collector polls go2rtc every 500 ms, downloading
    /// new MPEG-TS segments. If go2rtc is unavailable it retries automatically.
    pub fn start_ring(&self, camera_ip: &str) {
        {
            let rings = self.ring_sessions.lock().unwrap();
            if rings.contains_key(camera_ip) {
                return; // already collecting
            }
        }

        let ip_key = camera_ip.replace('.', "_");
        let seg_dir = self.ring_dir.join(&ip_key);
        if let Err(e) = fs::create_dir_all(&seg_dir) {
            log::warn!("ring[{camera_ip}]: mkdir failed: {e}");
            return;
        }

        // Clear stale segments when no recording is active. With mtime-based
        // live-segment collection a fresh start is always safe and avoids
        // stale-segment confusion from the previous run.
        let has_active_recording = self.recording_locks.lock().unwrap().contains_key(camera_ip);
        if !has_active_recording {
            if let Ok(rd) = fs::read_dir(&seg_dir) {
                for entry in rd.filter_map(|e| e.ok()) {
                    if entry.path().extension().map(|x| x == "ts").unwrap_or(false) {
                        let _ = fs::remove_file(entry.path());
                    }
                }
            }
        }

        let (stop_tx, stop_rx) = std::sync::mpsc::sync_channel(1);

        let ip_t          = camera_ip.to_string();
        let seg_dir_t     = seg_dir.clone();
        let pre_roll_secs = self.config.pre_roll_secs;
        let locks         = Arc::clone(&self.recording_locks);
        let ring_sessions = Arc::clone(&self.ring_sessions);

        std::thread::spawn(move || {
            run_hls_collector(&ip_t, &seg_dir_t, pre_roll_secs, &locks, &ring_sessions, stop_rx);
        });

        self.ring_sessions.lock().unwrap()
            .insert(camera_ip.to_string(), RingSession { stop_tx });

        log::info!("ring[{camera_ip}]: started (max {pre_roll_secs}s buffer, go2rtc HLS)");
    }

    /// Stop the ring collector for `camera_ip`.
    pub fn stop_ring(&self, camera_ip: &str) {
        if let Some(rs) = self.ring_sessions.lock().unwrap().remove(camera_ip) {
            let _ = rs.stop_tx.try_send(());
        }
    }

    // ---- recording ----------------------------------------------------------

    pub fn start(&self, camera_ip: &str, requested_pre_roll_secs: u32) -> Result<(String, bool), String> {
        // Quick duplicate check (lock released before start_ring).
        {
            let sessions = self.sessions.lock().unwrap();
            if let Some((id, _)) = sessions.iter().find(|(_, s)| s.camera_ip == camera_ip) {
                return Ok((id.clone(), false));
            }
        }

        let ts = chrono::Utc::now();
        let epoch = ts.timestamp();
        let ip_key = camera_ip.replace('.', "_");
        let clip_id = format!("{ip_key}-{epoch}");
        let started_at = ts.to_rfc3339();
        let output = self.dir.join(format!("{clip_id}.mp4"));

        // Ensure ring is running before we snapshot pre-roll.
        self.start_ring(camera_ip);

        // Second check after ring start (another thread could have won the race).
        let mut sessions = self.sessions.lock().unwrap();
        if let Some((id, _)) = sessions.iter().find(|(_, s)| s.camera_ip == camera_ip) {
            return Ok((id.clone(), false));
        }

        let max_pre_roll = requested_pre_roll_secs.min(self.config.pre_roll_secs);
        let ring_cam_dir = self.ring_dir.join(&ip_key);
        let ring_segs = collect_preroll(&ring_cam_dir, max_pre_roll);
        let preroll: Vec<PathBuf> = ring_segs.iter().enumerate().filter_map(|(i, src)| {
            let dst = self.live_dir.join(format!("{clip_id}_pre_{i:03}.ts"));
            if let Err(e) = fs::copy(src, &dst) {
                log::warn!("recording[{clip_id}]: copy preroll seg {i}: {e}");
                None
            } else {
                Some(dst)
            }
        }).collect();

        if !preroll.is_empty() {
            log::info!("recording[{clip_id}]: pre-roll {}s ({} segments)", preroll.len(), preroll.len());
        } else if requested_pre_roll_secs > 0 {
            log::warn!("recording[{clip_id}]: requested {requested_pre_roll_secs}s pre-roll but ring has no segments yet");
        }

        // All ring segments with mtime >= this moment are the live portion.
        let ring_started_at = SystemTime::now();
        self.recording_locks.lock().unwrap().insert(camera_ip.to_string(), ring_started_at);

        log::info!("recording started: {clip_id}");
        sessions.insert(clip_id.clone(), Session {
            clip_id: clip_id.clone(),
            camera_ip: camera_ip.to_string(),
            output,
            preroll,
            ring_started_at,
            started_at,
        });
        Ok((clip_id, true))
    }

    pub fn stop(&self, clip_id: &str) -> Result<ClipMeta, String> {
        let mut sessions = self.sessions.lock().unwrap();
        let sess = sessions.remove(clip_id)
            .ok_or_else(|| format!("no active session for clip_id {clip_id}"))?;

        let ring_cam_dir = self.ring_dir.join(sess.camera_ip.replace('.', "_"));
        let live_segs = collect_live_segs(&ring_cam_dir, sess.ring_started_at);

        if live_segs.is_empty() && sess.preroll.is_empty() {
            self.recording_locks.lock().unwrap().remove(&sess.camera_ip);
            return Err(format!("no footage captured for {clip_id} — ring may not have had segments yet"));
        }

        let mut sources: Vec<PathBuf> = sess.preroll.iter().cloned().collect();
        sources.extend(live_segs);

        let size_bytes = if sources.len() == 1 {
            remux_ts_to_mp4(&sources[0], &sess.output)?;
            fs::metadata(&sess.output).map(|m| m.len()).unwrap_or(0) as i64
        } else {
            let src_refs: Vec<&PathBuf> = sources.iter().collect();
            concat_to_mp4(&src_refs, &sess.output)?;
            fs::metadata(&sess.output).map(|m| m.len()).unwrap_or(0) as i64
        };

        self.recording_locks.lock().unwrap().remove(&sess.camera_ip);

        for p in &sess.preroll {
            let _ = fs::remove_file(p);
        }

        let started = chrono::DateTime::parse_from_rfc3339(&sess.started_at)
            .map(|t| t.with_timezone(&chrono::Utc))
            .unwrap_or_else(|_| chrono::Utc::now());
        let duration_secs = probe_duration(&sess.output)
            .unwrap_or_else(|| (chrono::Utc::now() - started).num_seconds().max(0) as i32);

        log::info!("recording stopped: {clip_id} ({duration_secs}s, {size_bytes}B)");
        if size_bytes < 1000 {
            log::warn!("recording[{clip_id}]: clip is only {size_bytes}B — stream may not have been warm");
        }

        let friendly_name = generate_friendly_name(started.timestamp());

        let sidecar = serde_json::json!({
            "clip_id":       &sess.clip_id,
            "friendly_name": &friendly_name,
            "camera_ip":     &sess.camera_ip,
            "duration_secs": duration_secs,
            "size_bytes":    size_bytes,
            "started_at":    &sess.started_at,
        });
        let sidecar_path = self.dir.join(format!("{clip_id}.json"));
        if let Err(e) = fs::write(&sidecar_path, sidecar.to_string()) {
            log::warn!("recording[{clip_id}]: sidecar write failed: {e}");
        }

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
        let clip_id = self.sessions.lock().unwrap()
            .iter()
            .find(|(_, s)| s.camera_ip == camera_ip)
            .map(|(id, _)| id.clone());
        clip_id.map(|id| self.stop(&id))
    }

    pub fn clip_path(&self, clip_id: &str) -> Option<PathBuf> {
        let candidate = self.dir.join(format!("{clip_id}.mp4"));
        if candidate.exists() { Some(candidate) } else { None }
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
                    if &c.camera_ip != ip { return false; }
                }
                if let Some(ref from) = p.from {
                    if c.started_at.as_str() < from.as_str() { return false; }
                }
                if let Some(ref to) = p.to {
                    if c.started_at.as_str() > to.as_str() { return false; }
                }
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
        let items = items.into_iter().skip(p.offset).take(p.limit).collect();
        ClipPage { total, items }
    }

    pub fn storage_stats(&self) -> StorageStats {
        StorageStats::compute(&self.dir, self.config.max_storage_mb)
    }

    pub fn evict_if_over_limit(&self) -> usize {
        let max_bytes = self.config.max_storage_mb.saturating_mul(1024 * 1024);
        if max_bytes == 0 { return 0; }
        let target = (max_bytes as f64 * 0.80) as u64;

        let mut clips: Vec<(String, u64, String)> = fs::read_dir(&self.dir)
            .into_iter()
            .flatten()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().map(|x| x == "json").unwrap_or(false))
            .filter_map(|e| {
                let raw = fs::read_to_string(e.path()).ok()?;
                let v = serde_json::from_str::<serde_json::Value>(&raw).ok()?;
                let clip_id    = v.get("clip_id")?.as_str()?.to_string();
                let size_bytes = v.get("size_bytes").and_then(|b| b.as_u64()).unwrap_or(0);
                let started_at = v.get("started_at").and_then(|s| s.as_str()).unwrap_or("").to_string();
                Some((clip_id, size_bytes, started_at))
            })
            .collect();
        clips.sort_by(|a, b| a.2.cmp(&b.2));

        let total: u64 = clips.iter().map(|(_, sz, _)| sz).sum();
        if total <= max_bytes { return 0; }

        let active = self.sessions.lock().unwrap()
            .values()
            .map(|s| s.clip_id.clone())
            .collect::<std::collections::HashSet<_>>();

        let mut remaining = total;
        let mut deleted = 0;
        for (clip_id, size_bytes, _) in &clips {
            if remaining <= target { break; }
            if active.contains(clip_id) { continue; }
            if self.delete_clip(clip_id).is_ok() {
                remaining = remaining.saturating_sub(*size_bytes);
                deleted += 1;
                log::info!("storage evict: deleted {clip_id} ({size_bytes}B) — {remaining}/{max_bytes}B");
            }
        }
        deleted
    }

    pub fn delete_clip(&self, clip_id: &str) -> Result<(), String> {
        let mp4  = self.dir.join(format!("{clip_id}.mp4"));
        let json = self.dir.join(format!("{clip_id}.json"));
        if !mp4.exists() {
            return Err(format!("clip not found: {clip_id}"));
        }
        fs::remove_file(&mp4).map_err(|e| format!("remove mp4: {e}"))?;
        let _ = fs::remove_file(&json);
        Ok(())
    }
}

// ---- go2rtc HLS ring collector ----------------------------------------------

/// Background thread: polls go2rtc's HLS API for `camera_ip`, downloads new
/// MPEG-TS segments to `seg_dir`, and evicts old ones. Exits when the stop
/// signal fires or the camera's entry is removed from `ring_sessions`.
fn run_hls_collector(
    camera_ip:       &str,
    seg_dir:         &PathBuf,
    pre_roll_secs:   u32,
    recording_locks: &Arc<Mutex<HashMap<String, SystemTime>>>,
    ring_sessions:   &Arc<Mutex<HashMap<String, RingSession>>>,
    stop_rx:         std::sync::mpsc::Receiver<()>,
) {
    let ip_key      = camera_ip.replace('.', "_");
    let stream_name = format!("cam_{ip_key}");
    let go2rtc_base = app_core::go2rtc::BASE;
    let master_url  = format!("{go2rtc_base}/api/stream.m3u8?src={stream_name}");
    let keep        = pre_roll_secs as usize + 2;

    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(5))
        .build();

    let mut session_id: Option<String> = None;
    // Last go2rtc segment index we downloaded in the current session.
    let mut last_n: i64 = -1;
    // Our own monotonically-increasing disk segment counter (never resets).
    let mut disk_n: u64 = 0;

    log::debug!("ring-hls[{camera_ip}]: collector running");

    loop {
        std::thread::sleep(Duration::from_millis(500));

        // Stop if signalled or entry removed from ring_sessions.
        if stop_rx.try_recv().is_ok() || !ring_sessions.lock().unwrap().contains_key(camera_ip) {
            log::debug!("ring-hls[{camera_ip}]: collector exiting");
            break;
        }

        // --- Ensure we have a live HLS session ---
        let sid = match &session_id {
            Some(s) => s.clone(),
            None => {
                let body = match agent.get(&master_url).call().and_then(|r| r.into_string().map_err(Into::into)) {
                    Ok(b) => b,
                    Err(e) => {
                        log::warn!("ring-hls[{camera_ip}]: go2rtc unavailable: {e}");
                        continue;
                    }
                };
                // Master playlist: "hls/playlist.m3u8?id=ABC123"
                let maybe_id = body.lines()
                    .find(|l| l.contains("id="))
                    .and_then(|l| l.split("id=").nth(1))
                    .map(|s| s.trim().to_string());
                match maybe_id {
                    Some(id) => {
                        last_n = -1; // new session → restart segment sequence
                        session_id = Some(id.clone());
                        id
                    }
                    None => {
                        log::warn!("ring-hls[{camera_ip}]: could not parse session from master playlist");
                        continue;
                    }
                }
            }
        };

        // --- Fetch segment playlist ---
        let playlist_url = format!("{go2rtc_base}/api/hls/playlist.m3u8?id={sid}");
        let playlist = match agent.get(&playlist_url).call().and_then(|r| r.into_string().map_err(Into::into)) {
            Ok(b) => b,
            Err(_) => {
                // Session expired or go2rtc restarted — create a new one next iteration.
                session_id = None;
                continue;
            }
        };

        // --- Download any new segments ---
        // Segment lines look like: "segment.ts?id=ABC123&n=5"
        let new_ns: Vec<u64> = playlist.lines()
            .filter(|l| l.contains("segment.ts") && l.contains("&n="))
            .filter_map(|l| {
                l.split("&n=").nth(1)
                    .and_then(|s| s.trim().parse::<u64>().ok())
            })
            .filter(|&n| n as i64 > last_n)
            .collect();

        for n in new_ns {
            let seg_url = format!("{go2rtc_base}/api/hls/segment.ts?id={sid}&n={n}");
            match agent.get(&seg_url).call() {
                Ok(resp) => {
                    let mut data = Vec::new();
                    if resp.into_reader().read_to_end(&mut data).is_ok() && data.len() > 500 {
                        let dst = seg_dir.join(format!("seg_{disk_n:06}.ts"));
                        if fs::write(&dst, &data).is_ok() {
                            disk_n += 1;
                        }
                    }
                    // Always advance last_n so we don't retry the same index.
                    last_n = n as i64;
                }
                Err(_) => break, // segment not ready yet — try again next poll
            }
        }

        // --- Evict old segments beyond the rolling window ---
        let min_time = recording_locks.lock().unwrap().get(camera_ip).copied();
        cleanup_ring_dir(seg_dir, keep, camera_ip, min_time);
    }
}

// ---- helpers ----------------------------------------------------------------

/// Parse the numeric index from a ring segment filename (`seg_000123.ts` → 123).
fn seg_idx(path: &PathBuf) -> Option<u64> {
    path.file_stem()
        .and_then(|s| s.to_str())
        .and_then(|s| s.strip_prefix("seg_"))
        .and_then(|s| s.parse().ok())
}

/// Collect the last `max_secs` complete segments from the ring dir, sorted oldest-first.
/// Drops the highest-indexed segment (may still be writing).
fn collect_preroll(ring_dir: &PathBuf, max_secs: u32) -> Vec<PathBuf> {
    if max_secs == 0 || !ring_dir.exists() {
        return vec![];
    }
    let mut entries: Vec<(u64, PathBuf)> = fs::read_dir(ring_dir)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map(|x| x == "ts").unwrap_or(false))
        .filter_map(|e| seg_idx(&e.path()).map(|idx| (idx, e.path())))
        .collect();

    entries.sort_by_key(|(idx, _)| *idx);

    if entries.len() > 1 {
        entries.pop(); // drop highest-indexed (may still be downloading)
    }

    let start = entries.len().saturating_sub(max_secs as usize);
    entries[start..].iter().map(|(_, p)| p.clone()).collect()
}

/// Collect ring segments with mtime >= `started_at`. These are the live portion
/// of the clip. Drops the most-recently-modified segment (may still be writing).
fn collect_live_segs(ring_dir: &PathBuf, started_at: SystemTime) -> Vec<PathBuf> {
    if !ring_dir.exists() {
        return vec![];
    }

    let mut entries: Vec<(SystemTime, PathBuf)> = fs::read_dir(ring_dir)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map(|x| x == "ts").unwrap_or(false))
        .filter_map(|e| {
            let mtime = e.metadata().ok()?.modified().ok()?;
            if mtime >= started_at { Some((mtime, e.path())) } else { None }
        })
        .collect();

    entries.sort_by_key(|(mtime, _)| *mtime);

    if entries.len() > 1 {
        entries.pop(); // drop most-recent (may still be downloading)
    }

    log::debug!("collect_live_segs: {} segments since {:?}", entries.len(), started_at);
    entries.into_iter().map(|(_, p)| p).collect()
}

/// Evict old ring segments, keeping the `keep` most recent by mtime.
/// Never deletes segments with mtime >= `min_keep_time` (active recording window).
fn cleanup_ring_dir(dir: &PathBuf, keep: usize, ip: &str, min_keep_time: Option<SystemTime>) {
    let mut entries: Vec<(SystemTime, PathBuf)> = match fs::read_dir(dir) {
        Ok(rd) => rd.filter_map(|e| e.ok())
            .filter(|e| e.path().extension().map(|x| x == "ts").unwrap_or(false))
            .filter_map(|e| {
                let mtime = e.metadata().ok()?.modified().ok()?;
                Some((mtime, e.path()))
            })
            .collect(),
        Err(_) => return,
    };

    // Protect segments written during an active recording.
    if let Some(min_time) = min_keep_time {
        entries.retain(|(mtime, _)| *mtime < min_time);
    }

    if entries.len() <= keep {
        return;
    }

    entries.sort_by_key(|(mtime, _)| *mtime);
    let to_delete = entries.len() - keep;
    for (_, path) in entries.iter().take(to_delete) {
        if let Err(e) = fs::remove_file(path) {
            log::debug!("ring cleanup[{ip}]: remove {:?}: {e}", path);
        }
    }
    log::debug!("ring cleanup[{ip}]: removed {to_delete} old segment(s)");
}

/// Remux a single .ts → .mp4.
fn remux_ts_to_mp4(src: &PathBuf, dst: &PathBuf) -> Result<(), String> {
    let status = Command::new("ffmpeg")
        .args([
            "-loglevel", "warning",
            "-i", src.to_str().unwrap_or_default(),
            "-c", "copy",
            "-movflags", "+faststart",
            "-y",
            dst.to_str().unwrap_or_default(),
        ])
        .status()
        .map_err(|e| format!("remux: {e}"))?;
    if status.success() { Ok(()) } else { Err("remux failed".to_string()) }
}

/// Run ffprobe to get the exact clip duration.
fn probe_duration(path: &PathBuf) -> Option<i32> {
    let out = Command::new("ffprobe")
        .args([
            "-v", "error",
            "-show_entries", "format=duration",
            "-of", "default=noprint_wrappers=1:nokey=1",
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

/// Concatenate multiple .ts files → .mp4 using ffmpeg concat demuxer.
fn concat_to_mp4(sources: &[&PathBuf], dst: &PathBuf) -> Result<(), String> {
    let list_path = dst.with_extension("concat.txt");
    let mut content = String::new();
    for src in sources {
        content.push_str(&format!("file '{}'\n", src.display()));
    }
    fs::write(&list_path, &content).map_err(|e| format!("concat list: {e}"))?;

    let status = Command::new("ffmpeg")
        .args([
            "-loglevel", "warning",
            "-f", "concat",
            "-safe", "0",
            "-i", list_path.to_str().unwrap_or_default(),
            "-c", "copy",
            "-movflags", "+faststart",
            "-y",
            dst.to_str().unwrap_or_default(),
        ])
        .status()
        .map_err(|e| format!("concat: {e}"))?;

    let _ = fs::remove_file(&list_path);
    if status.success() { Ok(()) } else { Err("concat failed".to_string()) }
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
