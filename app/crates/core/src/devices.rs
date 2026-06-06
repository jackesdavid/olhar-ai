//! The saved/discovered camera devices the app manages. Copied from the ratatui
//! `src/scan/domain.rs` — the `Device` struct, the `DeviceStore` shared-list
//! alias, and `devices.json` load/save. The sonar `Tracker`, `ScanState`, and
//! all ratatui bits are dropped; persistence is path-parameterized (no hardcoded
//! `devices.json` in the cwd) so the Tauri shell writes under the OS app-data dir.
use std::path::Path;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use crate::onvif::DiscoveredDevice;

/// The shared device list — held by the Tauri managed state and cloned into
/// background threads so the network commands read/mutate the same store.
pub type DeviceStore = Arc<Mutex<Vec<Device>>>;

/// A camera the operator has scanned (and optionally saved credentials for).
/// Persisted to `devices.json` so it survives restarts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Device {
    /// Stable ONVIF device id (WS-Addressing `EndpointReference`, e.g.
    /// `urn:uuid:…`). The reconciler keys on this so a camera that changes IP
    /// (DHCP / power-cycle) is recognised as the same device. Empty for devices
    /// saved before this existed — backfilled on the next scan by IP match.
    #[serde(default)]
    pub id: String,
    pub ip: String,
    pub xaddr: String,
    pub name: String,
    pub manufacturer: Option<String>,
    pub model: Option<String>,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub password: Option<String>,
    /// RTSP URL returned when the credentials were validated.
    #[serde(default)]
    pub rtsp_url: Option<String>,
}

impl Device {
    pub fn from_discovered(d: DiscoveredDevice) -> Self {
        Device {
            id: d.id,
            ip: d.ip,
            xaddr: d.xaddr,
            name: d.name,
            manufacturer: d.manufacturer,
            model: d.model,
            username: None,
            password: None,
            rtsp_url: None,
        }
    }

    pub fn has_credentials(&self) -> bool {
        self.username.is_some() && self.password.is_some()
    }
}

/// Load the saved device list from `path`. Returns an empty list if the file is
/// missing or unreadable.
pub fn load_devices(path: &Path) -> Vec<Device> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// Persist the device list to `path` (pretty JSON). Best-effort: write errors
/// are swallowed, matching the ratatui app's behavior.
pub fn save_devices(path: &Path, devices: &[Device]) {
    if let Ok(json) = serde_json::to_string_pretty(devices) {
        let _ = std::fs::write(path, json);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk(ip: &str) -> Device {
        Device {
            id: String::new(),
            ip: ip.into(),
            xaddr: String::new(),
            name: ip.into(),
            manufacturer: None,
            model: None,
            username: None,
            password: None,
            rtsp_url: None,
        }
    }

    #[test]
    fn round_trips_through_a_path() {
        let dir = std::env::temp_dir().join("app_core_devices_test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("devices.json");
        let devices = vec![mk("10.0.0.1"), mk("10.0.0.2")];
        save_devices(&path, &devices);
        let loaded = load_devices(&path);
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].ip, "10.0.0.1");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn missing_file_is_empty() {
        let path = std::env::temp_dir().join("app_core_devices_nonexistent_xyz.json");
        let _ = std::fs::remove_file(&path);
        assert!(load_devices(&path).is_empty());
    }
}
