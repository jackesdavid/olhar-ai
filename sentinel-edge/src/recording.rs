//! Ring-buffer + on-demand clip recording for the edge.
//!
//! One ffmpeg process per camera keeps a rolling ring of 1-second TS segments.
//! When a workflow node calls `POST /recording/start`, we spawn a second ffmpeg
//! muxer that reads the go2rtc RTSP feed and writes concatenated segments to a
//! dated MP4 file under `EDGE_RECORDING_DIR`.
//!
//! At `POST /recording/stop` (or `GET /recording/<id>`) the clip is finalised
//! and a push is sent to control so the cloud knows a new recording is available.
//!
//! All timestamps are RFC3339. clip_id = `<ip>-<epoch_secs>`.
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Per-box ring buffer + recording config, fetched from control at startup.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct RecordingConfig {
    /// Pre-roll ring size in seconds (number of 1-second TS segments to keep).
    #[serde(default = "default_pre_roll")]
    pub pre_roll_secs: u32,
    /// Max total storage for all clips in MB.
    #[serde(default = "default_storage")]
    pub max_storage_mb: u64,
    /// ffmpeg video codec for saved clips.
    #[serde(default = "default_codec")]
    pub codec: String,
}

impl Default for RecordingConfig {
    fn default() -> Self {
        Self { pre_roll_secs: 5, max_storage_mb: 1000, codec: "copy".to_string() }
    }
}

fn default_pre_roll() -> u32 { 5 }
fn default_storage() -> u64 { 1000 }
fn default_codec() -> String { "copy".to_string() }

/// A live recording session for one camera.
struct Session {
    clip_id:    String,
    camera_ip:  String,
    output:     PathBuf,
    started_at: String,
    child:      Option<Child>,
}

/// Manager: one per edge process. Holds the config + active sessions.
pub struct RecordingManager {
    pub config: RecordingConfig,
    dir:        PathBuf,
    sessions:   Mutex<HashMap<String, Session>>, // key = clip_id
}

impl RecordingManager {
    /// Load config from control (BOX_TOKEN auth). Falls back to defaults if control is unreachable.
    pub fn boot(control_url: &str) -> Arc<RecordingManager> {
        let config = fetch_config(control_url).unwrap_or_default();
        let dir = std::env::var("EDGE_RECORDING_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("/tmp/sentinel-recordings"));
        if let Err(e) = std::fs::create_dir_all(&dir) {
            log::warn!("recording dir {:?}: {e}", dir);
        }
        log::info!("Recording dir {:?}, pre_roll={}s", dir, config.pre_roll_secs);
        Arc::new(RecordingManager { config, dir, sessions: Mutex::new(HashMap::new()) })
    }

    /// Start recording `camera_ip` from its go2rtc RTSP feed.
    /// Returns the `clip_id` so the caller can `stop` it later.
    /// Idempotent: if a session for this IP is already active, returns its clip_id.
    pub fn start(&self, camera_ip: &str) -> Result<String, String> {
        let mut sessions = self.sessions.lock().unwrap();

        // Idempotency: return existing session.
        if let Some((id, _)) = sessions.iter().find(|(_, s)| s.camera_ip == camera_ip) {
            return Ok(id.clone());
        }

        let ts = chrono::Utc::now();
        let epoch = ts.timestamp();
        let clip_id = format!("{}-{epoch}", camera_ip.replace('.', "_"));
        let started_at = ts.to_rfc3339();
        let filename = format!("{clip_id}.mp4");
        let output = self.dir.join(&filename);

        // go2rtc exposes each camera's RTSP feed at rtsp://localhost:8554/cam_<ip>
        // where dots in the IP are replaced by underscores.
        let feed_ip = camera_ip.replace('.', "_");
        let rtsp_url = format!("rtsp://localhost:8554/cam_{feed_ip}");

        let child = Command::new("ffmpeg")
            .args([
                "-loglevel", "error",
                "-rtsp_transport", "tcp",
                "-i", &rtsp_url,
                "-c:v", &self.config.codec,
                "-an",          // skip audio (most ONVIF cams don't send it anyway)
                "-y",           // overwrite if exists
                output.to_str().unwrap_or(&filename),
            ])
            .spawn()
            .map_err(|e| format!("ffmpeg spawn failed for {camera_ip}: {e}"))?;

        let sess = Session {
            clip_id: clip_id.clone(),
            camera_ip: camera_ip.to_string(),
            output,
            started_at,
            child: Some(child),
        };
        sessions.insert(clip_id.clone(), sess);
        log::info!("recording started: {clip_id}");
        Ok(clip_id)
    }

    /// Stop the recording session for `clip_id`. Returns clip metadata to be pushed to control.
    pub fn stop(&self, clip_id: &str) -> Result<ClipMeta, String> {
        let mut sessions = self.sessions.lock().unwrap();
        let mut sess = sessions.remove(clip_id)
            .ok_or_else(|| format!("no active session for clip_id {clip_id}"))?;

        // Gracefully terminate ffmpeg.
        if let Some(mut child) = sess.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }

        let size_bytes = std::fs::metadata(&sess.output).map(|m| m.len()).unwrap_or(0) as i64;
        let started = chrono::DateTime::parse_from_rfc3339(&sess.started_at)
            .map(|t| t.with_timezone(&chrono::Utc))
            .unwrap_or_else(|_| chrono::Utc::now());
        let duration_secs = (chrono::Utc::now() - started).num_seconds().max(0) as i32;
        log::info!("recording stopped: {clip_id} ({duration_secs}s, {size_bytes}B)");

        Ok(ClipMeta {
            clip_id: sess.clip_id,
            camera_ip: sess.camera_ip,
            duration_secs,
            size_bytes,
            started_at: sess.started_at,
            output: sess.output,
        })
    }

    /// Stop by camera IP (for the workflow `StopRecording` node when clip_id is unknown).
    pub fn stop_by_ip(&self, camera_ip: &str) -> Option<Result<ClipMeta, String>> {
        let clip_id = self.sessions.lock().unwrap()
            .iter()
            .find(|(_, s)| s.camera_ip == camera_ip)
            .map(|(id, _)| id.clone());
        clip_id.map(|id| self.stop(&id))
    }

    /// Serve a clip file as raw bytes for the control download proxy.
    pub fn clip_path(&self, clip_id: &str) -> Option<PathBuf> {
        let candidate = self.dir.join(format!("{clip_id}.mp4"));
        if candidate.exists() { Some(candidate) } else { None }
    }
}

/// Metadata for a finalised clip.
pub struct ClipMeta {
    pub clip_id:       String,
    pub camera_ip:     String,
    pub duration_secs: i32,
    pub size_bytes:    i64,
    pub started_at:    String,
    pub output:        PathBuf,
}

/// Fetch the recording config from control using the BOX_TOKEN.
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
