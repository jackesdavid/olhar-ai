//! Shared edge state + go2rtc lifecycle. The edge is **config-light**: it has no
//! local database. Its camera list is pulled from the control plane (`CONTROL_URL`)
//! and cached to a local file so it survives a control-plane outage / offline boot.
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
}

impl AppState {
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
        log::info!("Edge has {} camera(s) (control={control_url})", devices.len());

        let own_go2rtc = std::env::var("EDGE_OWN_GO2RTC")
            .map(|v| !matches!(v.trim(), "0" | "false" | "no"))
            .unwrap_or(true);
        let go2rtc_config = std::env::var("GO2RTC_CONFIG")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("go2rtc.yaml"));
        let go2rtc_bin = std::env::var("GO2RTC_BIN")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("go2rtc"));

        Arc::new(AppState {
            control_url,
            devices: Mutex::new(devices),
            cache_path,
            go2rtc: Mutex::new(None),
            own_go2rtc,
            go2rtc_config,
            go2rtc_bin,
        })
    }

    /// Resolve a saved camera by exact IP.
    pub fn device_by_ip(&self, ip: &str) -> Option<Device> {
        self.devices.lock().unwrap().iter().find(|d| d.ip == ip).cloned()
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
        n
    }

    /// Run an ONVIF WS-Discovery sweep on the LAN and push the results up to control,
    /// so the cloud Devices page can surface them. The edge dials OUT — no inbound to
    /// the box is ever needed. Best-effort; tagged with the box token + edge version.
    pub fn push_discovered(&self) {
        let found = app_core::onvif::discovery::discover();
        let n = found.len();
        let url = format!("{}/config/discovered", self.control_url);
        let agent = ureq::AgentBuilder::new().timeout(Duration::from_secs(15)).build();
        let mut req = agent.post(&url);
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
        req = req.set("X-Edge-Version", env!("CARGO_PKG_VERSION"));
        match req.send_json(&found) {
            Ok(_) => log::info!("discovery: pushed {n} camera(s) to control"),
            Err(e) => log::warn!("discovery push failed: {e}"),
        }
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
            // Merge the edge-local creds + resolved RTSP into freshly-pulled cameras that
            // arrive without them (control hands creds over once; RTSP stays edge-only).
            for d in devices.iter_mut() {
                if let Some(prev) = cached.iter().find(|p| p.ip == d.ip) {
                    if d.username.as_deref().map(|u| u.trim().is_empty()).unwrap_or(true)
                        && prev.username.as_deref().map(|u| !u.trim().is_empty()).unwrap_or(false)
                    {
                        d.username = prev.username.clone();
                        d.password = prev.password.clone();
                    }
                    if d.rtsp_url.as_deref().map(|u| u.trim().is_empty()).unwrap_or(true) {
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
        let needs = d.rtsp_url.as_deref().map(|u| u.trim().is_empty()).unwrap_or(true);
        let has_creds = d.username.as_deref().map(|u| !u.trim().is_empty()).unwrap_or(false)
            && d.password.as_deref().map(|p| !p.is_empty()).unwrap_or(false);
        if !(needs && has_creds) {
            continue;
        }
        let target = if d.xaddr.trim().is_empty() { d.ip.clone() } else { d.xaddr.clone() };
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
    req.call()
        .map_err(|e| format!("{e}"))?
        .into_json::<Vec<Device>>()
        .map_err(|e| format!("parse cameras: {e}"))
}
