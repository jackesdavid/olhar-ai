//! On-demand clip recording with pre-roll ring buffer.
//!
//! Architecture:
//!
//! **Ring buffer** (`start_ring` / `stop_ring`)
//!   A background ffmpeg per camera writes 1-second MPEG-TS segments to
//!   `/tmp/sentinel-ring/{ip}/seg_%06d.ts`. A cleanup thread keeps only the
//!   last `pre_roll_secs + 2` segments (the +2 guards against the segment
//!   currently being written). Started by `warm_cameras()` on edge boot.
//!
//! **Recording** (`start` / `stop`)
//!   `start(ip, requested_secs)` copies the last N complete ring segments as
//!   the pre-roll, notes the current ring segment index, and sets a "recording
//!   lock" so the cleanup thread won't delete new segments while recording.
//!   `stop(clip_id)` collects ring segments written since start(), then runs
//!   ffmpeg to concat [preroll copies + live ring segments] into the final .mp4.
//!
//!   This design avoids spawning a second ffmpeg for the live portion: the ring
//!   buffer is ALREADY writing the camera stream continuously, so its segments
//!   are used directly. This eliminates the "gap" between pre-roll and live that
//!   occurs when a second ffmpeg process takes time to connect to the RTSP stream.
//!   Every ring segment starts at a keyframe (guaranteed by the segment muxer),
//!   so the join between pre-roll and live is seamless.
//!
//! **Graceful shutdown**
//!   The ring ffmpeg receives 'q\n' on stdin to flush and write the final segment.
//!   A 5-second deadline + SIGKILL fallback ensures stop() never hangs.
//!
//! All ffmpeg stderr is forwarded to the edge log buffer (→ cloud log viewer).

use std::collections::HashMap;
use std::fs;
use std::io::Write as _;
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

// ---- config -----------------------------------------------------------------

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct RecordingConfig {
    /// Maximum ring buffer size in seconds — also the upper bound for pre_roll_secs
    /// in the StartRecording node. Drives how many segments warm_cameras() keeps.
    #[serde(default = "default_pre_roll")]
    pub pre_roll_secs: u32,
    #[serde(default = "default_storage")]
    pub max_storage_mb: u64,
    #[serde(default = "default_codec")]
    pub codec: String,
    /// Keep every camera's RTSP stream open permanently via a ring buffer so
    /// recordings start with pre-roll footage instead of from the trigger moment.
    #[serde(default)]
    pub cameras_always_warm: bool,
}

impl Default for RecordingConfig {
    fn default() -> Self {
        Self {
            pre_roll_secs: 10,
            max_storage_mb: 1000,
            codec: "copy".to_string(),
            cameras_always_warm: false,
        }
    }
}

fn default_pre_roll() -> u32 { 10 }
fn default_storage() -> u64 { 1000 }
fn default_codec() -> String { "copy".to_string() }

// ---- ring buffer ------------------------------------------------------------

#[allow(dead_code)]
struct RingSession {
    child: Option<Child>,
    stdin: Option<ChildStdin>,
}

// ---- live session -----------------------------------------------------------

struct Session {
    clip_id:       String,
    camera_ip:     String,
    /// Final .mp4 output path.
    output:        PathBuf,
    /// Pre-roll segment copies (in live_dir, safe from ring cleanup).
    preroll:       Vec<PathBuf>,
    /// First ring segment index that belongs to the "live" portion.
    start_seg_idx: u64,
    started_at:    String,
}

// ---- manager ----------------------------------------------------------------

pub struct RecordingManager {
    pub config: RecordingConfig,
    /// Final clip directory.
    dir:          PathBuf,
    /// Ring segment root: {ring_dir}/{ip}/seg_%06d.ts
    ring_dir:     PathBuf,
    /// Pre-roll copies while recording (intermediate stash before concat).
    live_dir:     PathBuf,
    sessions:     Mutex<HashMap<String, Session>>,
    ring_sessions: Mutex<HashMap<String, RingSession>>,
    /// Per-camera minimum ring segment index to preserve during active recording.
    /// Cleanup thread checks this before deleting segments.
    recording_locks: Arc<Mutex<HashMap<String, u64>>>,
}

impl RecordingManager {
    pub fn boot(control_url: &str) -> Arc<RecordingManager> {
        let config = fetch_config(control_url).unwrap_or_default();
        // Default to a persistent path that survives edge restarts and OS reboots.
        let dir = std::env::var("EDGE_RECORDING_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("/opt/sentinel-edge/recordings"));
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
            sessions: Mutex::new(HashMap::new()),
            ring_sessions: Mutex::new(HashMap::new()),
            recording_locks: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    // ---- ring ---------------------------------------------------------------

    /// Start the ring buffer for `camera_ip`. Idempotent — no-op if already running.
    pub fn start_ring(&self, camera_ip: &str) {
        let mut rings = self.ring_sessions.lock().unwrap();
        if rings.contains_key(camera_ip) {
            return;
        }

        let ip_key = camera_ip.replace('.', "_");
        let seg_dir = self.ring_dir.join(&ip_key);
        if let Err(e) = fs::create_dir_all(&seg_dir) {
            log::warn!("ring[{camera_ip}]: mkdir failed: {e}");
            return;
        }

        // Remove stale segments from any previous ring run. ffmpeg always starts
        // its segment counter from 000000, so old high-numbered segments would
        // confuse index-based ordering (cleanup would keep stale old ones and
        // delete the fresh new ones).
        if let Ok(rd) = fs::read_dir(&seg_dir) {
            for entry in rd.filter_map(|e| e.ok()) {
                if entry.path().extension().map(|x| x == "ts").unwrap_or(false) {
                    let _ = fs::remove_file(entry.path());
                }
            }
        }

        let rtsp_url = format!("rtsp://localhost:8554/cam_{ip_key}");
        let seg_pattern = seg_dir.join("seg_%06d.ts");

        let mut cmd = Command::new("ffmpeg");
        cmd.args([
                "-loglevel", "warning",
                "-rtsp_transport", "tcp",
                "-i", &rtsp_url,
                "-c:v", "copy",
                "-an",
                "-f", "segment",
                "-segment_time", "1",
                "-segment_format", "mpegts",
                "-reset_timestamps", "1",
                seg_pattern.to_str().unwrap_or_default(),
            ])
            .stdin(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => { log::warn!("ring[{camera_ip}]: spawn failed: {e}"); return; }
        };
        let stdin = child.stdin.take();

        if let Some(stderr) = child.stderr.take() {
            let ip = camera_ip.to_string();
            std::thread::spawn(move || {
                use std::io::BufRead as _;
                for line in std::io::BufReader::new(stderr).lines().map_while(Result::ok) {
                    log::warn!("ring-ffmpeg[{ip}]: {line}");
                }
            });
        }

        log::info!("ring[{camera_ip}]: started (max {}s buffer)", self.config.pre_roll_secs);
        rings.insert(camera_ip.to_string(), RingSession { child: Some(child), stdin });

        // Cleanup thread: keep only the last (pre_roll_secs + 2) segments,
        // but never delete segments at or above an active recording's start index.
        let keep = self.config.pre_roll_secs as usize + 2;
        let dir_clone = seg_dir.clone();
        let ip_clone = camera_ip.to_string();
        let locks = Arc::clone(&self.recording_locks);
        std::thread::spawn(move || {
            loop {
                std::thread::sleep(Duration::from_secs(2));
                let min_idx = locks.lock().unwrap().get(&ip_clone).copied();
                cleanup_ring_dir(&dir_clone, keep, &ip_clone, min_idx);
            }
        });
    }

    /// Stop the ring buffer for `camera_ip`.
    pub fn stop_ring(&self, camera_ip: &str) {
        let mut sess = self.ring_sessions.lock().unwrap().remove(camera_ip);
        if let Some(mut rs) = sess.take() {
            kill_ffmpeg(&mut rs.child, &mut rs.stdin, &format!("ring[{camera_ip}]"));
        }
    }

    // ---- recording ----------------------------------------------------------

    /// Start recording `camera_ip`. Collects up to `requested_pre_roll_secs` of
    /// ring footage to prepend; the live portion is captured from the ring buffer
    /// directly (no second ffmpeg process), ensuring seamless keyframe-aligned join.
    pub fn start(&self, camera_ip: &str, requested_pre_roll_secs: u32) -> Result<String, String> {
        let mut sessions = self.sessions.lock().unwrap();
        if let Some((id, _)) = sessions.iter().find(|(_, s)| s.camera_ip == camera_ip) {
            return Ok(id.clone());
        }

        let ts = chrono::Utc::now();
        let epoch = ts.timestamp();
        let ip_key = camera_ip.replace('.', "_");
        let clip_id = format!("{ip_key}-{epoch}");
        let started_at = ts.to_rfc3339();
        let output = self.dir.join(format!("{clip_id}.mp4"));

        // Collect available pre-roll segments and copy them into live_dir so the
        // ring cleanup loop can't delete them before stop() concatenates them.
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
            log::warn!("recording[{clip_id}]: requested {requested_pre_roll_secs}s pre-roll but ring has no segments");
        }

        // Note which ring segment index marks the start of the "live" portion.
        // The cleanup thread will not delete segments at or above this index.
        let start_seg_idx = max_ring_seg_idx(&ring_cam_dir).map(|n| n + 1).unwrap_or(0);
        self.recording_locks.lock().unwrap().insert(camera_ip.to_string(), start_seg_idx);

        log::info!("recording started: {clip_id} (ring-only, start_seg={start_seg_idx})");
        sessions.insert(clip_id.clone(), Session {
            clip_id: clip_id.clone(),
            camera_ip: camera_ip.to_string(),
            output,
            preroll,
            start_seg_idx,
            started_at,
        });
        Ok(clip_id)
    }

    pub fn stop(&self, clip_id: &str) -> Result<ClipMeta, String> {
        let mut sessions = self.sessions.lock().unwrap();
        let sess = sessions.remove(clip_id)
            .ok_or_else(|| format!("no active session for clip_id {clip_id}"))?;

        // Collect ring segments written since start() — these form the live portion.
        let ring_cam_dir = self.ring_dir.join(sess.camera_ip.replace('.', "_"));
        let live_segs = collect_live_segs(&ring_cam_dir, sess.start_seg_idx);

        if live_segs.is_empty() && sess.preroll.is_empty() {
            self.recording_locks.lock().unwrap().remove(&sess.camera_ip);
            return Err(format!("no footage captured for {clip_id} — ring may not have been running"));
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

        // Release the recording lock — cleanup can now delete those live ring segments.
        self.recording_locks.lock().unwrap().remove(&sess.camera_ip);

        // Clean up copied pre-roll stash.
        for p in &sess.preroll {
            let _ = fs::remove_file(p);
        }

        let started = chrono::DateTime::parse_from_rfc3339(&sess.started_at)
            .map(|t| t.with_timezone(&chrono::Utc))
            .unwrap_or_else(|_| chrono::Utc::now());
        let duration_secs = (chrono::Utc::now() - started).num_seconds().max(0) as i32;

        log::info!("recording stopped: {clip_id} ({duration_secs}s, {size_bytes}B)");
        if size_bytes < 1000 {
            log::warn!("recording[{clip_id}]: clip is only {size_bytes}B — stream may not have been warm");
        }

        Ok(ClipMeta {
            clip_id: sess.clip_id,
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
}

// ---- helpers ----------------------------------------------------------------

/// Parse the numeric index from a ring segment filename (`seg_000123.ts` → 123).
fn seg_idx(path: &PathBuf) -> Option<u64> {
    path.file_stem()
        .and_then(|s| s.to_str())
        .and_then(|s| s.strip_prefix("seg_"))
        .and_then(|s| s.parse().ok())
}

/// Highest segment index currently in `ring_dir`, or None if no segments exist.
fn max_ring_seg_idx(ring_dir: &PathBuf) -> Option<u64> {
    fs::read_dir(ring_dir).ok()?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map(|x| x == "ts").unwrap_or(false))
        .filter_map(|e| seg_idx(&e.path()))
        .max()
}

/// Collect the last `max_secs` complete segment files from the ring dir.
/// "Complete" means not the highest-indexed (still being written by ffmpeg).
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

    // Drop the last entry — it's the segment currently being written by ffmpeg.
    if entries.len() > 1 {
        entries.pop();
    }

    // Take the last max_secs entries.
    let start = entries.len().saturating_sub(max_secs as usize);
    entries[start..].iter().map(|(_, p)| p.clone()).collect()
}

/// Collect ring segments at or above `start_idx`, excluding the one currently
/// being written (highest index in the dir at call time).
fn collect_live_segs(ring_dir: &PathBuf, start_idx: u64) -> Vec<PathBuf> {
    if !ring_dir.exists() {
        return vec![];
    }
    let mut entries: Vec<(u64, PathBuf)> = fs::read_dir(ring_dir)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map(|x| x == "ts").unwrap_or(false))
        .filter_map(|e| seg_idx(&e.path()).map(|idx| (idx, e.path())))
        .filter(|(idx, _)| *idx >= start_idx)
        .collect();

    entries.sort_by_key(|(idx, _)| *idx);

    // Drop the highest-indexed segment — it may still be open for writing.
    if entries.len() > 1 {
        entries.pop();
    }

    log::debug!(
        "collect_live_segs(start={start_idx}): {} segments [{:?}..{:?}]",
        entries.len(),
        entries.first().map(|(i, _)| i),
        entries.last().map(|(i, _)| i),
    );

    entries.into_iter().map(|(_, p)| p).collect()
}

/// Delete old ring segments, keeping the `keep` most recent.
/// Never deletes segments with index >= `min_keep_idx` (active recording window).
fn cleanup_ring_dir(dir: &PathBuf, keep: usize, ip: &str, min_keep_idx: Option<u64>) {
    let mut entries: Vec<(u64, PathBuf)> = match fs::read_dir(dir) {
        Ok(rd) => rd.filter_map(|e| e.ok())
            .filter(|e| e.path().extension().map(|x| x == "ts").unwrap_or(false))
            .filter_map(|e| seg_idx(&e.path()).map(|idx| (idx, e.path())))
            .collect(),
        Err(_) => return,
    };

    // If recording is active, protect segments at or above its start index.
    if let Some(min_idx) = min_keep_idx {
        entries.retain(|(idx, _)| *idx < min_idx);
    }

    if entries.len() <= keep {
        return;
    }
    entries.sort_by_key(|(idx, _)| *idx);
    let to_delete = entries.len() - keep;
    for (_, path) in entries.iter().take(to_delete) {
        if let Err(e) = fs::remove_file(path) {
            log::debug!("ring cleanup[{ip}]: remove {:?}: {e}", path);
        }
    }
    log::debug!("ring cleanup[{ip}]: removed {to_delete} old segment(s)");
}

/// Gracefully stop an ffmpeg child: send 'q\n' on stdin, wait up to 5s, then SIGKILL.
fn kill_ffmpeg(child: &mut Option<Child>, stdin: &mut Option<ChildStdin>, label: &str) {
    if let Some(mut s) = stdin.take() {
        let _ = s.write_all(b"q\n");
    }
    if let Some(mut c) = child.take() {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            match c.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) if std::time::Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(100));
                }
                _ => {
                    log::warn!("{label}: did not exit in 5s — SIGKILL");
                    let _ = c.kill();
                    let _ = c.wait();
                    break;
                }
            }
        }
    }
}

/// Remux a single .ts → .mp4 (no pre-roll, avoids a concat file).
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
