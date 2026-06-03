//! The motion-tracker sweep plus the discovered/saved camera devices the scan
//! feature manages. (Contacts are drawn from the device list, not stored here.)
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use crate::onvif::DiscoveredDevice;

/// The shared device list — held by `ScanState` and cloned into the agent thread
/// so chat tools can read/mutate the same store as the UI.
pub type DeviceStore = Arc<Mutex<Vec<Device>>>;

pub const TAU: f64 = std::f64::consts::TAU;
pub const SWEEP_SPEED: f64 = 0.06; // radians per tick

/// The rotating sonar sweep — a single angle reused as the radar ray, the device
/// pulse, the boot cursor blink, and the top-bar animation phase.
pub struct Tracker {
    pub sweep: f64,
}

impl Tracker {
    pub fn new() -> Self {
        Tracker { sweep: 0.0 }
    }
}

impl Default for Tracker {
    fn default() -> Self {
        Self::new()
    }
}

// -- Devices -----------------------------------------------------------------

/// A camera the operator has scanned (and optionally saved credentials for).
/// Persisted to `devices.json` so it survives restarts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Device {
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

/// The scan feature's state: the sonar, the shared saved/scanned device list,
/// the current selection, and whether a discovery sweep is in progress.
pub struct ScanState {
    pub tracker: Tracker,
    pub devices: DeviceStore,
    pub selected: usize,
    pub scanning: bool,
}

impl ScanState {
    pub fn new() -> Self {
        ScanState {
            tracker: Tracker::new(),
            devices: Arc::new(Mutex::new(load_devices())),
            selected: 0,
            scanning: false,
        }
    }

    /// The sonar animates only while scanning or when contacts are on file.
    pub fn is_active(&self) -> bool {
        self.scanning || !self.devices.lock().unwrap().is_empty()
    }

    pub fn device_count(&self) -> usize {
        self.devices.lock().unwrap().len()
    }

    /// Owned clone of the selected device (the lock can't escape as a borrow).
    pub fn selected_device(&self) -> Option<Device> {
        self.devices.lock().unwrap().get(self.selected).cloned()
    }

    /// Owned clone of the device with the given IP, if present.
    pub fn device_by_ip(&self, ip: &str) -> Option<Device> {
        self.devices.lock().unwrap().iter().find(|d| d.ip == ip).cloned()
    }

    /// A snapshot of the device list for rendering (no lock held by the caller).
    pub fn devices_snapshot(&self) -> Vec<Device> {
        self.devices.lock().unwrap().clone()
    }
}

impl Default for ScanState {
    fn default() -> Self {
        Self::new()
    }
}

fn devices_path() -> PathBuf {
    PathBuf::from("devices.json")
}

pub fn load_devices() -> Vec<Device> {
    std::fs::read_to_string(devices_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save_devices(devices: &[Device]) {
    if let Ok(json) = serde_json::to_string_pretty(devices) {
        let _ = std::fs::write(devices_path(), json);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sweep_stays_in_range() {
        let mut tracker = Tracker::new();
        for _ in 0..1000 {
            tracker.tick();
            assert!(tracker.sweep >= 0.0 && tracker.sweep < TAU);
        }
    }

    #[test]
    fn merge_deduplicates_by_ip() {
        let mut scan = ScanState {
            tracker: Tracker::new(),
            devices: Arc::new(Mutex::new(vec![])),
            selected: 0,
            scanning: false,
        };
        let mk = |ip: &str| Device {
            ip: ip.into(),
            xaddr: String::new(),
            name: ip.into(),
            manufacturer: None,
            model: None,
            username: None,
            password: None,
            rtsp_url: None,
        };
        assert_eq!(scan.merge(vec![mk("10.0.0.1"), mk("10.0.0.2")]), 2);
        assert_eq!(scan.merge(vec![mk("10.0.0.2"), mk("10.0.0.3")]), 1); // .2 is a dup
        assert_eq!(scan.device_count(), 3);
    }
}
