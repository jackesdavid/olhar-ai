//! The saved/discovered camera devices the app manages. Copied from the ratatui
//! `src/scan/domain.rs` — the `Device` struct and the `DeviceStore` shared-list
//! alias. The sonar `Tracker`, `ScanState`, and all ratatui bits are dropped.
//! Persistence now lives in [`crate::db`] (Postgres); this module is just the
//! domain type and the shared in-memory store.
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use crate::onvif::DiscoveredDevice;

pub mod repo;

/// The shared device list — held by the Tauri managed state and cloned into
/// background threads so the network commands read/mutate the same store.
pub type DeviceStore = Arc<Mutex<Vec<Device>>>;

/// A camera the operator has scanned (and optionally saved credentials for).
/// Persisted to Postgres (the `devices` table) so it survives restarts.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
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
