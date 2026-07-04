//! Shared edge state + go2rtc lifecycle. The edge is **config-light**: it has no
//! local database. Its camera list is pulled from the control plane (`CONTROL_URL`)
//! and cached to a local file so it survives a control-plane outage / offline boot.
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Child;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use app_core::devices::Device;

/// Everything the HTTP handlers + go2rtc manager share.
pub struct AppState {
    /// The control-plane base URL the camera list is pulled from.
    pub control_url: String,
    /// The edge's cached camera list (resolved by ip on each snapshot), refreshed
    /// from the control plane at boot and on `POST /reload`.
    pub devices: Mutex<Vec<Device>>,
    /// Where the camera list is cached on disk (offline fallback).
    pub cache_path: PathBuf,
    /// The managed go2rtc child (when the edge owns the sidecar).
    pub go2rtc: Mutex<Option<Child>>,
    pub own_go2rtc: bool,
    pub go2rtc_config: PathBuf,
    pub go2rtc_bin: PathBuf,
    /// Ring-buffer + clip recording manager.
    pub recording: Arc<crate::recording::RecordingManager>,
    /// The tracker's live "state of the world" — each (camera, job class) worker overwrites its
    /// own entry every detection cycle (never partial-updates another job's), so `GET /tracks/:ip`
    /// can answer "what's tracked right now" without a per-request round trip to the tracker
    /// itself. Card: edge tracking — the `GetTrackedObjects` node's backing store.
    pub live_tracks: Mutex<HashMap<(String, String), Vec<LiveTrackOut>>>,
}

/// One currently-tracked entity, as reported by `GET /tracks/:ip` — mirrors
/// `app_core::workflows::domain::TrackedObjectInfo` (the node-facing shape control maps this
/// onto). `zone` is the zone's name if the job has one AND this track is currently inside it,
/// empty otherwise (no zone configured, or outside it).
#[derive(Debug, Clone, serde::Serialize)]
pub struct LiveTrackOut {
    pub uuid: String,
    pub class: String,
    pub bbox: [i32; 4],
    pub zone: String,
}

impl AppState {
    /// Returns the configured control-plane URL (same logic as `boot`).
    pub fn control_url() -> String {
        std::env::var("CONTROL_URL")
            .ok()
            .map(|s| s.trim().trim_end_matches('/').to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "http://127.0.0.1:8790".to_string())
    }

    /// Build the edge: pull the camera list from the control plane (falling back to
    /// the local cache when it's unreachable). No database connection.
    pub fn boot() -> Arc<AppState> {
        let control_url = std::env::var("CONTROL_URL")
            .ok()
            .map(|s| s.trim().trim_end_matches('/').to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "http://127.0.0.1:8790".to_string());
        let cache_path = std::env::var("EDGE_CACHE")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("cameras-cache.json"));

        let mut devices = load_cameras(&control_url, &cache_path, 0);
        resolve_rtsp(&mut devices);
        write_cache(&cache_path, &devices);
        log::info!(
            "Edge has {} camera(s) (control={control_url})",
            devices.len()
        );

        let own_go2rtc = std::env::var("EDGE_OWN_GO2RTC")
            .map(|v| !matches!(v.trim(), "0" | "false" | "no"))
            .unwrap_or(true);
        let go2rtc_config = std::env::var("GO2RTC_CONFIG")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("go2rtc.yaml"));
        let go2rtc_bin = std::env::var("GO2RTC_BIN")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("go2rtc"));

        let recording = crate::recording::RecordingManager::boot(&control_url);

        Arc::new(AppState {
            control_url,
            devices: Mutex::new(devices),
            cache_path,
            go2rtc: Mutex::new(None),
            own_go2rtc,
            go2rtc_config,
            go2rtc_bin,
            recording,
            live_tracks: Mutex::new(HashMap::new()),
        })
    }

    /// Resolve a saved camera by exact IP.
    pub fn device_by_ip(&self, ip: &str) -> Option<Device> {
        self.devices
            .lock()
            .unwrap()
            .iter()
            .find(|d| d.ip == ip)
            .cloned()
    }

    /// Re-pull the camera list (and restart go2rtc if owned). `wait` long-polls control:
    /// 0 returns immediately (manual reload), >0 blocks up to `wait`s until the web
    /// changes a camera — so edits apply in ~1s without a busy timer.
    pub fn reload(&self, wait: u64) -> usize {
        let mut fresh = load_cameras(&self.control_url, &self.cache_path, wait);
        resolve_rtsp(&mut fresh);
        write_cache(&self.cache_path, &fresh);
        let n = fresh.len();
        *self.devices.lock().unwrap() = fresh;
        if self.own_go2rtc {
            crate::go2rtc::respawn(self);
        }
        self.push_streaming();
        self.warm_cameras(); // restart any ring that crashed since last poll
        n
    }

    /// Re-run discovery AND reconcile cameras that changed IP (e.g. after a DHCP lease
    /// flip on reboot): for any cached camera whose stable ONVIF `id` is now seen at a
    /// different IP, repoint it, re-resolve its RTSP, regenerate go2rtc (`cam_<newip>`),
    /// and push the correction up so the cloud device row + Live page follow the move.
    /// Then push the full sweep to control's discovered list. Best-effort throughout.
    pub fn rediscover(&self) {
        let found = app_core::onvif::discovery::discover();
        let by_id: std::collections::HashMap<&str, (&str, &str)> = found
            .iter()
            .filter(|d| !d.id.trim().is_empty())
            .map(|d| (d.id.as_str(), (d.ip.as_str(), d.xaddr.as_str())))
            .collect();

        let mut moves: Vec<serde_json::Value> = Vec::new();
        {
            let mut devices = self.devices.lock().unwrap();
            for dev in devices.iter_mut() {
                if dev.id.trim().is_empty() {
                    continue;
                }
                if let Some((new_ip, new_xaddr)) = by_id.get(dev.id.as_str()) {
                    if !new_ip.is_empty() && *new_ip != dev.ip {
                        let old_ip = dev.ip.clone();
                        log::info!(
                            "camera '{}' moved {} -> {} (onvif id {})",
                            dev.name,
                            old_ip,
                            new_ip,
                            dev.id
                        );
                        moves.push(serde_json::json!({
                            "onvif_id": dev.id,
                            "old_ip": old_ip,
                            "new_ip": new_ip,
                            "xaddr": new_xaddr,
                        }));
                        dev.ip = new_ip.to_string();
                        dev.xaddr = new_xaddr.to_string();
                        dev.rtsp_url = None; // force re-resolve at the new address
                        dev.rtsp_sub_url = None;
                    }
                }
            }
            if !moves.is_empty() {
                resolve_rtsp(devices.as_mut_slice());
                write_cache(&self.cache_path, &devices);
            }
        }
        if !moves.is_empty() && self.own_go2rtc {
            crate::go2rtc::respawn(self);
        }
        if !moves.is_empty() {
            self.push_device_moves(&moves);
        }
        self.post_discovered(&found);
        self.push_streaming();
    }

    /// POST the current ONVIF sweep to control's discovered list (box-authed). Best-effort.
    fn post_discovered(&self, found: &[app_core::onvif::DiscoveredDevice]) {
        let n = found.len();
        let url = format!("{}/config/discovered", self.control_url);
        match self.box_agent().post(&url).send_json(found) {
            Ok(_) => log::info!("discovery: pushed {n} camera(s) to control"),
            Err(e) => log::warn!("discovery push failed: {e}"),
        }
    }

    /// Report cameras whose IP changed so the cloud follows the move (box-authed). Best-effort.
    fn push_device_moves(&self, moves: &[serde_json::Value]) {
        let url = format!("{}/config/device-moved", self.control_url);
        match self.box_agent().post(&url).send_json(moves) {
            Ok(_) => log::info!("reported {} camera IP move(s) to control", moves.len()),
            Err(e) => log::warn!("device-moved push failed: {e}"),
        }
    }

    /// Start the ring buffer for every camera with a resolved stream. Called on boot and
    /// after each rediscover so newly added cameras get a ring without a manual restart.
    /// Only runs when `cameras_always_warm = true` in the box recording config.
    pub fn warm_cameras(&self) {
        if !self.recording.config.cameras_always_warm {
            return;
        }
        let ips: Vec<String> = self
            .devices.lock().unwrap()
            .iter()
            .filter(|d| d.rtsp_url.as_deref().map(|u| !u.trim().is_empty()).unwrap_or(false))
            .map(|d| d.ip.clone())
            .collect();
        if ips.is_empty() {
            log::debug!("warm_cameras: no cameras with resolved streams");
            return;
        }
        for ip in &ips {
            self.recording.start_ring(ip);
        }
    }

    /// Report which cameras the edge currently has a resolved stream for (rtsp_url set →
    /// creds valid + camera reachable). Drives the cloud's "online & correct" status dot.
    /// Best-effort.
    pub fn push_streaming(&self) {
        let ips: Vec<String> = self
            .devices
            .lock()
            .unwrap()
            .iter()
            .filter(|d| {
                d.rtsp_url
                    .as_deref()
                    .map(|u| !u.trim().is_empty())
                    .unwrap_or(false)
            })
            .map(|d| d.ip.clone())
            .collect();
        let url = format!("{}/config/streaming", self.control_url);
        if let Err(e) = self.box_agent().post(&url).send_json(&ips) {
            log::debug!("streaming push failed: {e}");
        }
    }

    /// Report one normalized motion event to control (box-authed → tenant). The edge is a
    /// pure sensor: it sends the raw signal (foreground %, largest-blob area, bbox) for a
    /// camera; per-graph sensitivity is applied in the cloud (control → Redis → OnEvent).
    /// Best-effort.
    pub fn push_motion(&self, ev: &MotionEvent) {
        let url = format!("{}/config/motion", self.control_url);
        if let Err(e) = self.box_agent().post(&url).send_json(ev) {
            log::debug!("motion push failed: {e}");
        }
    }

    /// Push an object-tracker lifecycle event to control (box-authed) → durable event_queue →
    /// `OnEvent("object_entered"/"object_exited")`. Best-effort like motion.
    pub fn push_track_event(&self, ev: &TrackEventOut) {
        let url = format!("{}/config/track-event", self.control_url);
        if let Err(e) = self.box_agent().post(&url).send_json(ev) {
            log::debug!("track-event push failed: {e}");
        }
    }

    /// Poll control for pending commands (box-authed), consumed once per cycle.
    pub fn check_commands(&self) -> EdgeCommands {
        let url = format!("{}/config/commands", self.control_url);
        match self.box_agent().get(&url).call() {
            Ok(resp) => resp
                .into_json::<serde_json::Value>()
                .ok()
                .map(|v| EdgeCommands {
                    rescan:  v.get("rescan").and_then(|r| r.as_bool()).unwrap_or(false),
                    restart: v.get("restart").and_then(|r| r.as_bool()).unwrap_or(false),
                    update:  v.get("update").and_then(|r| r.as_bool()).unwrap_or(false),
                })
                .unwrap_or_default(),
            Err(_) => EdgeCommands::default(), // network hiccup — try again next cycle
        }
    }

    /// Push a batch of recent log lines to control (box-authed). Returns true on success.
    pub fn push_logs(&self, lines: &[crate::logbuf::LogLine]) -> bool {
        let url = format!("{}/config/logs", self.control_url);
        self.box_agent().post(&url).send_json(lines).is_ok()
    }

    /// Notify control that disk usage has crossed the warning threshold.
    /// Control publishes this as an `edge:full-disk` system event so tenant workflows can react.
    pub fn push_disk_alert(&self, used_bytes: u64, max_bytes: u64, pct: u8, path: &str) {
        let url = format!("{}/config/disk-alert", self.control_url);
        let body = serde_json::json!({
            "used_bytes": used_bytes,
            "max_bytes":  max_bytes,
            "pct":        pct,
            "path":       path,
        });
        if let Err(e) = self.box_agent().post(&url).send_json(&body) {
            log::debug!("disk alert push failed: {e}");
        }
    }

    /// A short-timeout ureq agent + request stamped with this box's token + tailnet host +
    /// edge version — the standard headers control uses to resolve the box's tenant.
    fn box_agent(&self) -> BoxReq {
        BoxReq {
            agent: ureq::AgentBuilder::new()
                .timeout(Duration::from_secs(15))
                .build(),
        }
    }
}

/// Pending cloud→edge commands, fetched from `/config/commands` each poll cycle.
#[derive(Default)]
pub struct EdgeCommands {
    pub rescan: bool,
    pub restart: bool,
    pub update: bool,
}

/// One motion observation the edge reports to control. Normalized signal only — the cloud
/// applies per-graph sensitivity. Coordinates/areas are in the watcher's processing
/// resolution (downscaled), so the cloud treats them as relative.
/// An object-tracker lifecycle event pushed to control's `/config/track-event`. Matches control's
/// `TrackEventIn`. `bbox` is canonical xyxy (source pixels); meaningless (`[0;4]`) for
/// `line_crossed` (carries `line`/`direction` instead) and for a zone-scoped `object_entered`/
/// `object_exited` (carries `zone` instead — see `Job::zone` in `tracking.rs`).
#[derive(serde::Serialize)]
pub struct TrackEventOut {
    pub kind: String, // "object_entered" | "object_exited" | "line_crossed"
    pub camera_ip: String,
    pub uuid: String,
    pub class: String,
    pub bbox: [i32; 4],
    pub count: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dwell_secs: Option<i64>,
    /// The crossed line's name (`line_crossed` only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<String>,
    /// `"forward"` | `"backward"`, relative to the line's own `p1→p2` direction (`line_crossed` only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub direction: Option<String>,
    /// The zone's name, when this `object_entered`/`object_exited` is zone-scoped (the job has a
    /// `Polygon` configured) rather than whole-frame.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub zone: Option<String>,
}

#[derive(serde::Serialize)]
pub struct MotionEvent {
    pub camera_ip: String,
    /// Foreground share of the frame, 0–100.
    pub score: f64,
    /// Largest motion blob area in pixels (processing resolution).
    pub area: i64,
    /// Number of qualifying motion blobs.
    pub blobs: i64,
    /// Largest blob bounding box [x, y, w, h] (processing resolution).
    pub bbox: [i32; 4],
    /// RFC3339 observation time.
    pub at: String,
    /// The JPEG frame that triggered this event (base64), so the cloud can deliver it
    /// as an `Image` port on `OnEvent("motion")` without a second round-trip to the edge.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frame_b64: Option<String>,
}

/// Whether `ffmpeg` is on PATH (go2rtc needs it for JPEG frames). Computed once + cached —
/// presence is effectively static for a process lifetime. Reported to the cloud for the
/// Boxes health badge.
pub fn ffmpeg_ok() -> bool {
    static OK: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *OK.get_or_init(|| {
        std::process::Command::new("ffmpeg")
            .arg("-version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    })
}

/// A friendly platform string (`OS (arch)`, e.g. `macOS (aarch64)` / `Bazzite (x86_64)`)
/// reported to control for the Boxes list. Computed once; the distro name comes from
/// `/etc/os-release` on Linux (so a Bazzite box reads as "Bazzite", not just "Linux").
///
/// MUST be ASCII — it rides an HTTP header (`X-Edge-Platform`); a non-ASCII separator (e.g.
/// `·`) makes ureq reject the whole request, silently breaking every heartbeat.
pub fn platform() -> &'static str {
    static P: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    P.get_or_init(|| {
        let arch = std::env::consts::ARCH;
        if cfg!(target_os = "macos") {
            format!("macOS ({arch})")
        } else if cfg!(target_os = "linux") {
            let distro = linux_distro().unwrap_or_else(|| "Linux".to_string());
            format!("{distro} ({arch})")
        } else {
            format!("{} ({arch})", std::env::consts::OS)
        }
    })
}

/// Best-effort friendly distro name from `/etc/os-release` (`NAME=…`), e.g. "Bazzite".
fn linux_distro() -> Option<String> {
    let text = std::fs::read_to_string("/etc/os-release").ok()?;
    text.lines()
        .find_map(|l| l.strip_prefix("NAME="))
        .map(|v| v.trim().trim_matches('"').to_string())
        .filter(|s| !s.is_empty())
}

/// Stamp the box's health (Tailscale state + ffmpeg presence + platform) onto a request so
/// control can surface it on the Boxes page. Used on the ~25s config poll (not the pushes).
fn with_health(req: ureq::Request) -> ureq::Request {
    req.set("X-Edge-Tailscale", crate::tailscale::tailscale_state())
        .set("X-Edge-Ffmpeg", if ffmpeg_ok() { "1" } else { "0" })
        .set("X-Edge-Platform", platform())
}

/// Tiny builder that stamps the box auth headers onto each edge→control request.
struct BoxReq {
    agent: ureq::Agent,
}
impl BoxReq {
    fn get(&self, url: &str) -> ureq::Request {
        Self::auth(self.agent.get(url))
    }
    fn post(&self, url: &str) -> ureq::Request {
        Self::auth(self.agent.post(url))
    }
    fn auth(mut req: ureq::Request) -> ureq::Request {
        if let Ok(t) = std::env::var("BOX_TOKEN") {
            let t = t.trim();
            if !t.is_empty() {
                req = req.set("Authorization", &format!("Bearer {t}"));
            }
        }
        if let Ok(h) = std::env::var("EDGE_PUBLIC_HOST") {
            let h = h.trim();
            if !h.is_empty() {
                req = req.set("X-Box-Host", h);
            }
        }
        req.set("X-Edge-Version", env!("CARGO_PKG_VERSION"))
    }
}

/// Pull the camera list from the control plane; on success refresh the on-disk cache,
/// on failure fall back to the cache (so an offline boot still serves the last list).
///
/// **Credentials live on the edge.** Control hands them over only once (the moment the
/// web sets them); thereafter its config has none. So we MERGE creds from our local
/// cache into any freshly-pulled camera that arrives without them — the edge is the
/// durable home for camera credentials, the cloud never persists them.
fn load_cameras(control_url: &str, cache_path: &PathBuf, wait: u64) -> Vec<Device> {
    let cached: Vec<Device> = std::fs::read(cache_path)
        .ok()
        .and_then(|b| serde_json::from_slice::<Vec<Device>>(&b).ok())
        .unwrap_or_default();
    match fetch_cameras(control_url, wait) {
        Ok(mut devices) => {
            // Resilience: the cache is the DURABLE, only home for camera credentials (control
            // hands them over once). Never let an empty pull wipe a populated cache — a transient
            // empty/misrouted 200 response (box-token hiccup, race) would otherwise destroy the
            // sole copy of the creds and leave the camera credential-less (→ go2rtc auth-lockout).
            // A real "all cameras removed" is rare and recoverable; losing creds is neither.
            if devices.is_empty() && !cached.is_empty() {
                log::warn!(
                    "control returned 0 cameras but cache has {} — keeping cache (creds are edge-only)",
                    cached.len()
                );
                return cached;
            }
            // Merge the edge-local creds + resolved RTSP into freshly-pulled cameras that
            // arrive without them (control hands creds over once; RTSP stays edge-only).
            for d in devices.iter_mut() {
                if let Some(prev) = cached.iter().find(|p| p.ip == d.ip) {
                    if d.username
                        .as_deref()
                        .map(|u| u.trim().is_empty())
                        .unwrap_or(true)
                        && prev
                            .username
                            .as_deref()
                            .map(|u| !u.trim().is_empty())
                            .unwrap_or(false)
                    {
                        d.username = prev.username.clone();
                        d.password = prev.password.clone();
                    }
                    if d.rtsp_url
                        .as_deref()
                        .map(|u| u.trim().is_empty())
                        .unwrap_or(true)
                    {
                        d.rtsp_url = prev.rtsp_url.clone();
                        d.rtsp_sub_url = prev.rtsp_sub_url.clone();
                    }
                }
            }
            devices
        }
        Err(e) => {
            log::warn!("control plane unreachable ({e}); using cached camera list");
            cached
        }
    }
}

/// Persist the current camera list (with edge-local creds + resolved RTSP) to disk so a
/// restart / offline boot keeps serving, and idle long-poll reconnects don't re-resolve.
fn write_cache(cache_path: &PathBuf, devices: &[Device]) {
    if let Ok(json) = serde_json::to_vec_pretty(devices) {
        if let Err(e) = std::fs::write(cache_path, json) {
            log::warn!("could not write camera cache {cache_path:?}: {e}");
        }
    }
}

/// For any camera that has credentials but no RTSP URL yet, resolve both the main and
/// substream RTSP URLs via ONVIF so go2rtc can serve an HD + a low-latency preview feed.
/// The URLs are **kept on the edge only** (a LAN concern, and they embed credentials) —
/// never reported to the cloud. Best-effort.
fn resolve_rtsp(devices: &mut [Device]) {
    for d in devices.iter_mut() {
        let needs = d
            .rtsp_url
            .as_deref()
            .map(|u| u.trim().is_empty())
            .unwrap_or(true);
        let has_creds = d
            .username
            .as_deref()
            .map(|u| !u.trim().is_empty())
            .unwrap_or(false)
            && d.password
                .as_deref()
                .map(|p| !p.is_empty())
                .unwrap_or(false);
        if !(needs && has_creds) {
            continue;
        }
        let target = if d.xaddr.trim().is_empty() {
            d.ip.clone()
        } else {
            d.xaddr.clone()
        };
        let user = d.username.clone().unwrap_or_default();
        let pass = d.password.clone().unwrap_or_default();
        match app_core::onvif::client::resolve_stream_uris(&target, &user, &pass) {
            Ok((main, sub)) => {
                log::info!(
                    "resolved rtsp for {} (edge-local, substream={})",
                    d.ip,
                    sub.is_some()
                );
                d.rtsp_url = Some(main);
                d.rtsp_sub_url = sub;
            }
            Err(e) => log::warn!("rtsp resolve failed for {}: {}", d.ip, e),
        }
    }
}

/// GET `{control_url}/config/cameras` → the camera list, scoped to this box's tenant.
/// Presents `BOX_TOKEN` (set by the installer) as a bearer so control returns only this
/// tenant's cameras, and reports `EDGE_PUBLIC_HOST` (this box's tailnet URL) so the
/// gateway can resolve the tenant's live upstream. Without a token, control falls back
/// to the default tenant (single-tenant / dev edges).
fn fetch_cameras(control_url: &str, wait: u64) -> Result<Vec<Device>, String> {
    let url = if wait > 0 {
        format!("{control_url}/config/cameras?wait={wait}")
    } else {
        format!("{control_url}/config/cameras")
    };
    let mut req = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(wait + 15)) // long enough to hold the long-poll
        .build()
        .get(&url);
    if let Ok(token) = std::env::var("BOX_TOKEN") {
        let token = token.trim();
        if !token.is_empty() {
            req = req.set("Authorization", &format!("Bearer {token}"));
        }
    }
    if let Ok(host) = std::env::var("EDGE_PUBLIC_HOST") {
        let host = host.trim();
        if !host.is_empty() {
            req = req.set("X-Box-Host", host);
        }
    }
    req = req.set("X-Edge-Version", env!("CARGO_PKG_VERSION"));
    req = with_health(req); // report Tailscale + ffmpeg health on the regular heartbeat
    req.call()
        .map_err(|e| format!("{e}"))?
        .into_json::<Vec<Device>>()
        .map_err(|e| format!("parse cameras: {e}"))
}
