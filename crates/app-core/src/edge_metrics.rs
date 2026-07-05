//! Edge resource-usage snapshot — CPU / memory / disk / GPU percentages the edge box
//! samples and ships to control on the heartbeat (the compact `X-Edge-Metrics` JSON header),
//! surfaced per-box on the Boxes page. Plain data (no sampling deps here): the edge builds it
//! via `sysinfo` + `nvidia-smi` (`sentinel-edge/src/metrics.rs`), control parses it off the
//! header and stamps `tenant_boxes`. All fields are whole-percent 0..=100.

use serde::{Deserialize, Serialize};

/// A point-in-time resource-usage snapshot for one edge box. GPU fields are `None` on boxes
/// without an NVIDIA GPU (no `nvidia-smi`), so the UI hides the GPU gauge there.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct EdgeMetrics {
    /// System-wide CPU utilization, percent.
    pub cpu: u8,
    /// Used memory as a percent of total.
    pub mem: u8,
    /// Used disk as a percent of the system/root filesystem.
    pub disk: u8,
    /// GPU utilization percent (NVIDIA only), absent otherwise.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub gpu: Option<u8>,
    /// GPU memory-used percent (NVIDIA only), absent otherwise.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub gpu_mem: Option<u8>,
}

impl EdgeMetrics {
    /// Serialize to the compact JSON carried in the ASCII `X-Edge-Metrics` header.
    pub fn to_header(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{}".to_string())
    }

    /// Parse the `X-Edge-Metrics` header value; `None` if absent/malformed.
    pub fn from_header(s: &str) -> Option<Self> {
        serde_json::from_str(s).ok()
    }
}
