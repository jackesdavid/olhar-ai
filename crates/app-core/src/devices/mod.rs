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
// `Default` exists for TEST FIXTURES: they construct a Device with the two or three fields the
// case is about and `..Default::default()` for the rest. Without it every fixture lists every
// field, so adding one — as #155 did with the four probe_* columns — breaks unrelated tests in
// files that have nothing to do with the change.
#[derive(Debug, Clone, Default, Serialize, Deserialize, sqlx::FromRow)]
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
    /// RTSP URL returned when the credentials were validated. This is the **main**
    /// (highest-resolution) profile's stream.
    #[serde(default)]
    pub rtsp_url: Option<String>,
    /// Live status (card #155, migration 0083) — `rtsp_probe::ProbeStatus` snake_case, plus the
    /// camera's own words and when it was established. NULL until first reported.
    ///
    /// These MUST be carried through `repo::save`, which is a delete-all + re-insert: a camera
    /// rename would otherwise silently wipe every camera's status in the tenant.
    #[serde(default)]
    pub probe_status: Option<String>,
    #[serde(default)]
    pub probe_detail: Option<String>,
    #[serde(default)]
    pub probe_at: Option<String>,
    #[serde(default)]
    pub last_frame_at: Option<String>,
    /// RTSP URL of the camera's **substream** (lowest-resolution profile), used for the
    /// low-latency preview feed. `None` for single-profile cameras. Edge-only (resolved
    /// via ONVIF, never persisted in the cloud) — `#[sqlx(default)]` so control's
    /// `devices` table needs no column for it.
    #[serde(default)]
    #[sqlx(default)]
    pub rtsp_sub_url: Option<String>,
    /// Stream mode (#120), a user choice persisted in the cloud `devices` table and pulled by
    /// the edge. `None`/`"split"` = today's behaviour (substream for preview/motion, main for
    /// HD/recording). `"unified"` = main-only: one camera session, motion runs on a downscaled
    /// copy of the main frame, faces are cropped from the full-res main frame, and no substream
    /// is pulled (avoids dual-stream contention on cameras that can't serve two profiles).
    #[serde(default)]
    pub stream_mode: Option<String>,
}

impl Device {
    /// True when this camera should run the whole pipeline off the main stream only.
    pub fn is_unified(&self) -> bool {
        matches!(self.stream_mode.as_deref(), Some("unified"))
    }
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
            rtsp_sub_url: None,
            stream_mode: None,
            // A just-discovered camera has no status yet, and NULL is the honest answer. It is also
            // load-bearing: control treats "no previous status" as "not a transition", so a new
            // camera never announces itself as having "come online".
            probe_status: None,
            probe_detail: None,
            probe_at: None,
            last_frame_at: None,
        }
    }

    pub fn has_credentials(&self) -> bool {
        self.username.is_some() && self.password.is_some()
    }
}
