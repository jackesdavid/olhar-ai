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

        let devices = load_cameras(&control_url, &cache_path);
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

    /// Re-pull the camera list from the control plane (and restart go2rtc if owned).
    pub fn reload(&self) -> usize {
        let fresh = load_cameras(&self.control_url, &self.cache_path);
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
fn load_cameras(control_url: &str, cache_path: &PathBuf) -> Vec<Device> {
    match fetch_cameras(control_url) {
        Ok(devices) => {
            if let Ok(json) = serde_json::to_vec_pretty(&devices) {
                if let Err(e) = std::fs::write(cache_path, json) {
                    log::warn!("could not write camera cache {cache_path:?}: {e}");
                }
            }
            devices
        }
        Err(e) => {
            log::warn!("control plane unreachable ({e}); using cached camera list");
            std::fs::read(cache_path)
                .ok()
                .and_then(|b| serde_json::from_slice::<Vec<Device>>(&b).ok())
                .unwrap_or_default()
        }
    }
}

/// GET `{control_url}/config/cameras` → the camera list, scoped to this box's tenant.
/// Presents `BOX_TOKEN` (set by the installer) as a bearer so control returns only this
/// tenant's cameras, and reports `EDGE_PUBLIC_HOST` (this box's tailnet URL) so the
/// gateway can resolve the tenant's live upstream. Without a token, control falls back
/// to the default tenant (single-tenant / dev edges).
fn fetch_cameras(control_url: &str) -> Result<Vec<Device>, String> {
    let url = format!("{control_url}/config/cameras");
    let mut req = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(8))
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
