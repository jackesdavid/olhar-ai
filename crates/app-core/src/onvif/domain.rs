//! Result of an ONVIF WS-Discovery probe.

use serde::{Deserialize, Serialize};

/// A camera that answered a discovery probe. No credentials yet — those are
/// added and validated later (see [`super::client`]).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveredDevice {
    /// Stable ONVIF device id (WS-Addressing `EndpointReference`, e.g.
    /// `urn:uuid:…`) — survives IP/DHCP changes. Empty if the camera omits it.
    #[serde(default)]
    pub id: String,
    /// ONVIF device-service URL, e.g. `http://192.168.1.50/onvif/device_service`.
    pub xaddr: String,
    /// Host extracted from `xaddr`.
    pub ip: String,
    /// Friendly name (hardware scope, falls back to `ip`).
    pub name: String,
    pub manufacturer: Option<String>,
    pub model: Option<String>,
}
