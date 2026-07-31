//! Shared edge state + go2rtc lifecycle. The edge is **config-light**: it has no
//! local database. Its camera list is pulled from the control plane (`CONTROL_URL`)
//! and cached to a local file so it survives a control-plane outage / offline boot.
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use app_core::devices::Device;

/// One camera in the `push_streaming` report — its resolved-stream IP plus the codec probed off
/// its own SDP, if known. `#[serde(rename_all)]` matches what `sentinel-control`'s
/// `/config/streaming` expects (`domain/devices.rs`, `store.rs`).
#[derive(serde::Serialize)]
pub struct StreamingCamera {
    pub ip: String,
    pub codec: Option<&'static str>,
}

/// Everything the HTTP handlers + go2rtc manager share.
pub struct AppState {
    /// The control-plane base URL the camera list is pulled from.
    pub control_url: String,
    /// The edge's cached camera list (resolved by ip on each snapshot), refreshed
    /// from the control plane at boot and on `POST /reload`.
    pub devices: Mutex<Vec<Device>>,
    /// Where the camera list is cached on disk (offline fallback).
    pub cache_path: PathBuf,
    /// Ring-buffer + clip recording manager.
    pub recording: Arc<crate::recording::RecordingManager>,
    /// Queue of finished clips waiting to be pushed to durable cloud storage. Single consumer —
    /// this box also serves live video, and parallel uploads would starve it.
    pub uploads: std::sync::mpsc::SyncSender<crate::clip_upload::Job>,
    /// The tracker's live "state of the world" — each (camera, job class) worker overwrites its
    /// own entry every detection cycle (never partial-updates another job's), so `GET /tracks/:ip`
    /// can answer "what's tracked right now" without a per-request round trip to the tracker
    /// itself. Card: edge tracking — the `GetTrackedObjects` node's backing store.
    pub live_tracks: Mutex<HashMap<(String, String), Vec<LiveTrackOut>>>,
    /// Last cold-probe result per camera IP: `(status, detail, when)`.
    ///
    /// Exists to stop the status loop re-dialing a camera that has told us it is refusing clients.
    /// `Locked` and `AuthFailed` are states that repeated connection attempts CAUSE — so a status
    /// check that re-dials every 60s does not observe the lockout, it sustains it. See
    /// `ProbeStatus::is_retryable`.
    pub probe_memo: Mutex<HashMap<String, ProbeMemo>>,
}

/// A remembered cold-probe result: what the camera said, and WHEN it said it.
///
/// `at` is monotonic and drives the cooldown; `checked_at` is wall-clock and is what the cloud
/// shows as the status age. They are separate because `Instant` cannot be turned into a date, and
/// the age is the part the user reads.
pub struct ProbeMemo {
    pub status: app_core::rtsp_probe::ProbeStatus,
    pub detail: String,
    pub at: Instant,
    pub checked_at: String,
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
        // The camera cache is the edge's durable, only home for camera credentials (control hands
        // them over once). It MUST land somewhere writable: a bare relative path lands in the CWD,
        // which iOS forbids (sandbox) — so default it under the platform base there (#144).
        let cache_path = std::env::var("EDGE_CACHE")
            .map(PathBuf::from)
            .unwrap_or_else(|_| crate::paths::cache_file());

        let mut devices = load_cameras(&control_url, &cache_path, 0);
        resolve_rtsp(&mut devices);
        write_cache(&cache_path, &devices);
        log::info!(
            "Edge has {} camera(s) (control={control_url})",
            devices.len()
        );

        let recording = crate::recording::RecordingManager::boot(&control_url);
        let uploads = crate::clip_upload::spawn(control_url.clone());

        Arc::new(AppState {
            control_url,
            devices: Mutex::new(devices),
            cache_path,
            probe_memo: Mutex::new(HashMap::new()),
            recording,
            uploads,
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
        match self.box_agent().send_signed_json(&url, found) {
            Ok(_) => log::info!("discovery: pushed {n} camera(s) to control"),
            Err(e) => log::warn!("discovery push failed: {e}"),
        }
    }

    /// Report cameras whose IP changed so the cloud follows the move (box-authed). Best-effort.
    fn push_device_moves(&self, moves: &[serde_json::Value]) {
        let url = format!("{}/config/device-moved", self.control_url);
        match self.box_agent().send_signed_json(&url, moves) {
            Ok(_) => log::info!("reported {} camera IP move(s) to control", moves.len()),
            Err(e) => log::warn!("device-moved push failed: {e}"),
        }
    }

    /// Start the ring buffer for every camera with a resolved stream. Called on boot and
    /// after each rediscover so newly added cameras get a ring without a manual restart.
    /// Only runs when `cameras_always_warm = true` in the box recording config.
    pub fn warm_cameras(&self) {
        // The current streamable set (rtsp_url resolved = creds valid + reachable).
        let streamable: std::collections::HashSet<String> = self
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

        // Publish where each ring should dial. The recording manager holds no devices and no
        // credentials, so this is the only place that can answer it — and it must be refreshed here,
        // where the device set is reconciled, or a camera that moved keeps dialling its old address.
        let dials: std::collections::HashMap<String, String> = streamable
            .iter()
            .filter_map(|ip| self.camera_rtsp(ip, false).map(|u| (ip.clone(), u)))
            .collect();
        self.recording.set_dials(dials);

        // ALWAYS reconcile — stop rings for cameras that left (IP change, removal, went
        // un-streamable), regardless of warm mode. This is #98: a ring used to outlive its camera
        // and retry a dead address forever, because start-only never subtracted. Runs on every
        // reload, so `ring_sessions` mirrors the device set.
        self.recording.reconcile_rings(&streamable);

        // Only in warm mode do we PRE-open a ring for each present camera; otherwise rings start
        // on demand at recording time.
        if self.recording.config.cameras_always_warm {
            for ip in &streamable {
                self.recording.start_ring(ip);
            }
        }
    }

    /// Report which cameras the edge currently has a resolved stream for (rtsp_url set →
    /// creds valid + camera reachable), plus each one's detected video codec when the
    /// gstreamer-linked pipeline has probed it (`None` on older backends or before the first
    /// probe). Drives the cloud's "online & correct" status dot and the Devices page's codec
    /// badge. Best-effort.
    /// Report each camera's stage to control, which turns health CHANGES into `camera_offline` /
    /// `camera_online` events (card #155).
    ///
    /// **Derived from the pipeline that is already running — this must NEVER open a second
    /// connection.** See the ONE-PIPELINE invariant in `capture.rs`: consumer cameras cap concurrent
    /// RTSP sessions at 2-4, and a status check that dials again is how a WORKING camera goes dark
    /// with "503 insufficient resource". So:
    ///
    ///   - frames arriving  → `streaming` (the only proof that actually proves it)
    ///   - no credentials   → `no_credentials`, answered without touching the network
    ///   - otherwise        → cold-probe, but ONLY for a camera with no pipeline to ask
    ///
    /// A camera whose pipeline is up but starved is `timeout`, not `unreachable`: we know we reached
    /// it, because we are connected to it.
    pub fn push_camera_status(&self) {
        use app_core::rtsp_probe::ProbeStatus;
        // Frames within this window count as live. Wider than the capture cadence so a single
        // dropped frame is not reported as an outage — flapping status is worse than none.
        const FRESH: Duration = Duration::from_secs(30);

        let devices = self.devices.lock().unwrap().clone();
        let mut out = Vec::with_capacity(devices.len());

        for d in &devices {
            let age = crate::capture::last_frame_age(&d.ip);
            // Set only when a status is RE-REPORTED from memory rather than freshly established.
            let mut established_at: Option<String> = None;
            let has_creds = !d.username.as_deref().unwrap_or("").trim().is_empty()
                || !d.password.as_deref().unwrap_or("").trim().is_empty();

            let (status, detail) = match age {
                // Proof of life. Nothing else needs asking.
                Some(a) if a <= FRESH => (ProbeStatus::Streaming, String::new()),
                // A pipeline exists but has gone quiet — we are connected, so this is the camera
                // stalling, not the network losing it.
                Some(a) => (
                    ProbeStatus::Timeout,
                    format!("no frame for {}s", a.as_secs()),
                ),
                None if !has_creds => (
                    ProbeStatus::NoCredentials,
                    "No username or password is set for this camera".to_string(),
                ),
                // No pipeline and no frames: this is the one case worth a dial, and the camera has
                // no session of ours to lose.
                None => match d.rtsp_url.as_deref().filter(|u| !u.trim().is_empty()) {
                    Some(url) => {
                        // A camera that answered `Locked` or `AuthFailed` must NOT be dialled again
                        // on the next 60s tick. Those states are produced BY repeated attempts:
                        // re-dialling does not observe the lockout, it renews it, and the camera can
                        // never age out of it while we keep knocking. Consumer cameras (Tapo, and
                        // most of the cheap ONVIF ones) hold the lock for minutes after the last
                        // attempt, so continuous probing is an indefinite outage we inflict.
                        //
                        // Not "never re-check": that would leave a power-cycled camera looking dead
                        // forever. Re-check on a long cooldown instead — slow enough to let the lock
                        // expire, quick enough that a fixed camera comes back on its own.
                        const LOCKED_COOLDOWN: Duration = Duration::from_secs(15 * 60);
                        let remembered = {
                            let memo = self.probe_memo.lock().unwrap();
                            memo.get(&d.ip).and_then(|m| {
                                let cooling =
                                    !m.status.is_retryable() && m.at.elapsed() < LOCKED_COOLDOWN;
                                cooling.then(|| (m.status, m.detail.clone(), m.checked_at.clone()))
                            })
                        };
                        if let Some((status, detail, checked_at)) = remembered {
                            // Carry the ORIGINAL check time up: this status is being re-reported,
                            // not re-established, and dating it "now" would claim a freshness the
                            // cooldown deliberately gave up.
                            established_at = Some(checked_at);
                            (status, detail)
                        } else {
                            let p = app_core::rtsp_probe::probe(
                                url,
                                d.username.as_deref().unwrap_or(""),
                                d.password.as_deref().unwrap_or(""),
                            );
                            let checked_at = app_core::timeutil::now_rfc3339();
                            self.probe_memo.lock().unwrap().insert(
                                d.ip.clone(),
                                ProbeMemo {
                                    status: p.status,
                                    detail: p.detail.clone(),
                                    at: Instant::now(),
                                    checked_at,
                                },
                            );
                            (p.status, p.detail)
                        }
                    }
                    None => (
                        ProbeStatus::NotFound,
                        "No stream URL resolved for this camera".to_string(),
                    ),
                },
            };

            out.push(serde_json::json!({
                "ip": d.ip,
                "status": status,
                "detail": detail,
                "last_frame_at": age.map(|a| {
                    app_core::timeutil::now_rfc3339_minus(a.as_secs() as i64)
                }),
                // Absent = established now (the common case: derived from the live pipeline).
                "checked_at": established_at,
            }));
        }

        let url = format!("{}/config/camera-status", self.control_url);
        if let Err(e) = self.box_agent().send_signed_json(&url, &out) {
            log::debug!("camera-status push failed: {e}");
        }
    }

    pub fn push_streaming(&self) {
        let cams: Vec<StreamingCamera> = self
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
            .map(|d| StreamingCamera {
                ip: d.ip.clone(),
                codec: crate::capture::codec_for(&d.ip),
            })
            .collect();
        let url = format!("{}/config/streaming", self.control_url);
        if let Err(e) = self.box_agent().send_signed_json(&url, &cams) {
            log::debug!("streaming push failed: {e}");
        }
    }

    /// Report one normalized motion event to control (box-authed → tenant). The edge is a
    /// pure sensor: it sends the raw signal (foreground %, largest-blob area, bbox) for a
    /// camera; per-graph sensitivity is applied in the cloud (control → Redis → OnEvent).
    /// Best-effort.
    pub fn push_motion(&self, ev: &MotionEvent) {
        let url = format!("{}/config/motion", self.control_url);
        if let Err(e) = self.box_agent().send_signed_json(&url, ev) {
            log::debug!("motion push failed: {e}");
        }
    }

    pub fn push_face(&self, ev: &FaceEvent) {
        let url = format!("{}/config/face", self.control_url);
        if let Err(e) = self.box_agent().send_signed_json(&url, ev) {
            log::debug!("face push failed: {e}");
        }
    }

    /// Push an object-tracker lifecycle event to control (box-authed) → durable event_queue →
    /// `OnEvent("object_entered"/"object_exited")`. Best-effort like motion.
    pub fn push_track_event(&self, ev: &TrackEventOut) {
        let url = format!("{}/config/track-event", self.control_url);
        if let Err(e) = self.box_agent().send_signed_json(&url, ev) {
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
                    rescan: v.get("rescan").and_then(|r| r.as_bool()).unwrap_or(false),
                    restart: v.get("restart").and_then(|r| r.as_bool()).unwrap_or(false),
                    update: v.get("update").and_then(|r| r.as_bool()).unwrap_or(false),
                })
                .unwrap_or_default(),
            Err(_) => EdgeCommands::default(), // network hiccup — try again next cycle
        }
    }

    /// Push a batch of recent log lines to control (box-authed). Returns true on success.
    pub fn push_logs(&self, lines: &[crate::logbuf::LogLine]) -> bool {
        let url = format!("{}/config/logs", self.control_url);
        self.box_agent().send_signed_json(&url, lines).is_ok()
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
        if let Err(e) = self.box_agent().send_signed_json(&url, &body) {
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

/// One face detection reported to control (#117) → `OnEvent("face_detected")`. `bbox` is canonical
/// xyxy in source pixels (YuNet already works in source space).
#[derive(serde::Serialize)]
pub struct FaceEvent {
    pub camera_ip: String,
    pub bbox: [i32; 4],  // xyxy
    pub confidence: f64, // 0..100
    pub face_width: i64, // face box width in source pixels (face-size signal)
    /// YuNet's 5 landmarks, FLAT `[x0,y0,x1,y1,…]` in source pixels (right/left eye, nose,
    /// right/left mouth). Flat so it survives the cloud's generic event-payload decoder → the
    /// `face_detected` `kps` Landmarks port → ArcFace alignment (#122). Empty if unavailable.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub kps: Vec<i32>,
    pub at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frame_b64: Option<String>,
}

impl AppState {
    /// The camera's **own** RTSP URL for the requested quality — what the edge dials now that it owns
    /// the ingest instead of proxying go2rtc's re-serve (card #138, G1/G5).
    ///
    /// Mirrors `go2rtc::config_yaml`'s stream choice exactly, because the ingest must land on the
    /// same stream the sidecar would have: substream for preview, main for HD, and main for both in
    /// unified mode (#120). Credentials are **not** stored in the URL — they live on the device and
    /// are injected here, the same way the sidecar config does it.
    pub fn camera_rtsp(&self, ip: &str, hd: bool) -> Option<String> {
        let devices = self.devices.lock().ok()?;
        pick_camera_rtsp(devices.iter().find(|d| d.ip == ip)?, hd)
    }
}

/// The dial URL for one device — split out from [`AppState::camera_rtsp`] so the credential
/// injection and the stream choice can be tested without a whole app state. Getting this wrong is
/// silent: the pipeline just reports `Unauthorized` and retries forever.
fn pick_camera_rtsp(d: &app_core::devices::Device, hd: bool) -> Option<String> {
    let rtsp = d.rtsp_url.as_deref()?;
    let user = d.username.as_deref().unwrap_or("");
    let pass = d.password.as_deref().unwrap_or("");
    // Never dial credential-less: blank auth (`rtsp://:@host/…`) trips some cameras'
    // anti-brute-force lock, and the reconnect storm then survives even correct credentials until
    // the camera reboots. Same guard the sidecar config carries, for the same reason.
    if !rtsp.contains('@') && (user.is_empty() || pass.is_empty()) {
        log::warn!(
            "ingest: no cached credentials for {} — not dialling (avoids auth-lockout)",
            d.ip
        );
        return None;
    }
    let main = app_core::onvif::client::rtsp_url_with_creds(rtsp, user, pass);
    let url = if hd || d.is_unified() {
        main
    } else {
        d.rtsp_sub_url
            .as_deref()
            .map(|s| app_core::onvif::client::rtsp_url_with_creds(s, user, pass))
            .unwrap_or(main)
    };
    Some(strip_stream_fragment(&clean_onvif_params(&url)))
}

/// Drop go2rtc's `#…` fragment from a stored RTSP URL.
fn strip_stream_fragment(url: &str) -> String {
    url.split('#').next().unwrap_or(url).trim().to_string()
}

/// Strip the ONVIF stream-setup hint params (`unicast`, `proto`) that ONVIF `GetStreamUri` appends
/// (`…?channel=1&subtype=0&unicast=true&proto=Onvif`). GStreamer's `rtspsrc` chokes on them — it
/// completes OPTIONS/auth then stalls at SETUP and loops — whereas the plain native path
/// (`…?channel=1&subtype=0`) that VLC/go2rtc use streams fine. Keep every other query param; only
/// the known ONVIF hints are removed. Query-less URLs pass through untouched.
fn clean_onvif_params(url: &str) -> String {
    let Some((base, query)) = url.split_once('?') else {
        return url.to_string();
    };
    let kept: Vec<&str> = query
        .split('&')
        .filter(|kv| {
            let key = kv.split('=').next().unwrap_or("").trim();
            !key.eq_ignore_ascii_case("unicast") && !key.eq_ignore_ascii_case("proto")
        })
        .collect();
    if kept.is_empty() {
        base.to_string()
    } else {
        format!("{base}?{}", kept.join("&"))
    }
}


/// Whether `gst-launch-1.0` is on PATH (card #138 — the GStreamer media backend option for
/// `capture.rs`'s live decode). Computed once, reported to the cloud so a
/// box can be told apart before flipping its `media_backend` to `"gstreamer"`. NOT bundled by
/// `install.sh` yet (coexistence phase) — an operator installs it manually to test
/// (`brew install gstreamer gst-plugins-{base,good,bad,ugly} gst-libav` on macOS).
pub fn gstreamer_ok() -> bool {
    // Always true, and not a lie: GStreamer is LINKED into this binary, so if the process is
    // running then its media path is present by construction. There is no `gst-launch-1.0` CLI to
    // probe for and no build that lacks it — the `gst-linked` feature is gone because there is
    // nothing left to select between.
    //
    // Kept as a function rather than deleted because control still reads it off the heartbeat to
    // tell boxes apart; it now answers "yes" everywhere instead of describing a fork.
    true
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
        } else if cfg!(target_os = "windows") {
            format!("Windows ({arch})")
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

/// Stamp the box's health (GStreamer presence + hwaccel + platform) onto a request so control can
/// surface it on the Boxes page. Used on the ~25s config poll (not the pushes). No more Tailscale or
/// ffmpeg probes — those binaries are retired, and running them every poll was wasted work + noise.
fn with_health(req: ureq::Request) -> ureq::Request {
    req.set("X-Edge-Gstreamer", if gstreamer_ok() { "1" } else { "0" })
        .set("X-Edge-Platform", platform())
        // Resource usage (CPU/mem/disk/GPU) as compact ASCII JSON — sampled off-thread.
        .set("X-Edge-Metrics", &crate::metrics::header_value())
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

    /// A signed GET.
    fn get_signed(&self, url: &str) -> ureq::Request {
        crate::signer::sign(self.get(url), "GET", url, b"")
    }

    /// POST a JSON body, signed.
    ///
    /// Cannot use `ureq`'s `send_json`: it serializes internally, so the caller never sees the bytes
    /// it has to hash — and a signature over a re-serialization that differs by even a space is a
    /// signature over something other than what was sent. Serialize once, hash and send the same
    /// buffer. Every signed POST on the edge must go through here for that reason.
    // `?Sized` so callers can pass slices (`&[Device]`, `&[LogLine]`) directly, which several do.
    fn send_signed_json<T: serde::Serialize + ?Sized>(
        &self,
        url: &str,
        body: &T,
    ) -> Result<ureq::Response, ureq::Error> {
        let bytes = serde_json::to_vec(body).unwrap_or_else(|_| b"null".to_vec());
        crate::signer::sign(self.post(url), "POST", url, &bytes)
            .set("Content-Type", "application/json")
            .send_bytes(&bytes)
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
    // Ensure the parent exists — on iOS the cache lives under a sandbox subdir that nothing else
    // creates (on the appliance the path is relative, so the parent is `.` and this is a no-op).
    if let Some(parent) = cache_path.parent().filter(|p| !p.as_os_str().is_empty()) {
        let _ = std::fs::create_dir_all(parent);
    }
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
/// Presents `BOX_TOKEN` (set by the installer, or by enrollment — see `crate::enroll`) as a bearer
/// so control returns only this tenant's cameras, and reports `EDGE_PUBLIC_HOST` (this box's tailnet
/// URL) so the gateway can resolve the tenant's live upstream.
///
/// **Without a token, control answers 401** — it does not fall back to any tenant. (It used to, and
/// this comment used to say so; the fallback is now compiled out of release builds entirely, since
/// this response carries RTSP credentials. See `sentinel-control`'s `untenanted_dev_config`.) A box
/// with no token never gets this far anyway: it stops at the enrollment page in `main`.
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
    req = with_health(req); // report GStreamer + platform + resource metrics on the heartbeat
                            // Signed last, so the `?wait=N` in the URL is covered and cannot be lengthened in flight. The
                            // health headers are deliberately outside the signature: they change every poll and are
                            // advisory display values, so binding them would make the canonical string fragile for nothing.
    req = crate::signer::sign(req, "GET", &url, b"");
    req.call()
        .map_err(|e| format!("{e}"))?
        .into_json::<Vec<Device>>()
        .map_err(|e| format!("parse cameras: {e}"))
}

#[cfg(test)]
mod camera_rtsp_tests {
    use super::{pick_camera_rtsp, strip_go2rtc_fragment};
    use app_core::devices::Device;

    fn cam(sub: Option<&str>, creds: bool) -> Device {
        let mut d = Device {
            id: String::new(),
            ip: "10.0.0.2".into(),
            xaddr: String::new(),
            name: "cam".into(),
            manufacturer: None,
            model: None,
            username: None,
            password: None,
            rtsp_url: Some("rtsp://10.0.0.2:554/stream1".into()),
            rtsp_sub_url: None,
            stream_mode: None,
        };
        d.rtsp_sub_url = sub.map(str::to_string);
        if creds {
            d.username = Some("u".into());
            d.password = Some("p".into());
        }
        d
    }

    /// The bug this suite exists for: the stored URL carries no credentials, so a dial built from it
    /// alone gets `Unauthorized` — and the pipeline just retries forever with no clue why.
    #[test]
    fn injects_the_devices_credentials_into_the_dial() {
        let got = pick_camera_rtsp(&cam(None, true), true).unwrap();
        assert!(got.contains("u:p@"), "credentials missing from {got}");
    }

    /// Preview must land on the substream and HD on main — the same choice the sidecar config makes,
    /// or the ingest quietly decodes a different stream than every consumer expects.
    #[test]
    fn preview_takes_the_substream_and_hd_takes_main() {
        let d = cam(Some("rtsp://10.0.0.2:554/stream2"), true);
        assert!(pick_camera_rtsp(&d, false).unwrap().ends_with("/stream2"));
        assert!(pick_camera_rtsp(&d, true).unwrap().ends_with("/stream1"));
    }

    #[test]
    fn falls_back_to_main_when_there_is_no_substream() {
        assert!(pick_camera_rtsp(&cam(None, true), false)
            .unwrap()
            .ends_with("/stream1"));
    }

    /// Unified mode (#120) means main-only — pulling the substream there is exactly the dual-stream
    /// contention it exists to avoid.
    #[test]
    fn unified_mode_never_touches_the_substream() {
        let mut d = cam(Some("rtsp://10.0.0.2:554/stream2"), true);
        d.stream_mode = Some("unified".into());
        assert!(pick_camera_rtsp(&d, false).unwrap().ends_with("/stream1"));
    }

    /// Dialling with blank auth can lock a camera out until it reboots — refusing is the safe answer.
    #[test]
    fn refuses_to_dial_without_credentials() {
        assert_eq!(pick_camera_rtsp(&cam(None, false), false), None);
    }

    /// `#media=video` is go2rtc's syntax, not RTSP. Left on, `rtspsrc` treats it as part of the path
    /// and the camera 404s — a failure that looks like bad credentials.
    #[test]
    fn strips_the_stream_fragment() {
        assert_eq!(
            strip_stream_fragment("rtsp://u:p@10.0.0.2:554/stream2#media=video"),
            "rtsp://u:p@10.0.0.2:554/stream2"
        );
    }

    #[test]
    fn leaves_a_plain_url_alone() {
        assert_eq!(
            strip_stream_fragment("rtsp://u:p@10.0.0.2:554/stream1"),
            "rtsp://u:p@10.0.0.2:554/stream1"
        );
    }
}
