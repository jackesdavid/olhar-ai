//! Settings — copied from `bishop::config::domain`, trimmed to the web-relevant fields
//! (the CRT/ratatui-only `crt_sweep*` fields are dropped). Persistence is
//! path-parameterized so the Tauri shell stores it under the OS app-config dir.

use serde::{Deserialize, Serialize};
use std::path::Path;

/// Smallest / largest chars-revealed-per-tick for the chat teletype.
pub const TYPING_MIN: u8 = 1;
pub const TYPING_MAX: u8 = 8;

/// User-tunable settings. Audio levels are 0.0..=1.0 channel gains (default 1.0 = original
/// loudness). Persisted as JSON.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub master: f32,
    pub voice: f32,
    pub ui: f32,
    pub ambient: f32,
    pub voice_enabled: bool,
    pub typing_speed: u8,
    pub stt_autosend: bool,
    /// RTSP transport for live camera feeds (used by the stream sidecar later).
    #[serde(default)]
    pub transport: Transport,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            master: 1.0,
            voice: 1.0,
            ui: 1.0,
            ambient: 1.0,
            voice_enabled: true,
            typing_speed: 2,
            stt_autosend: true,
            transport: Transport::Tcp,
        }
    }
}

/// Lower-layer transport for pulling an RTSP stream. TCP is the reliable default; UDP trades
/// reliability for latency; Auto negotiates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Transport {
    #[default]
    Tcp,
    Udp,
    Auto,
}

impl Transport {
    pub const ALL: [Transport; 3] = [Transport::Tcp, Transport::Udp, Transport::Auto];

    pub fn label(self) -> &'static str {
        match self {
            Transport::Tcp => "TCP",
            Transport::Udp => "UDP",
            Transport::Auto => "AUTO",
        }
    }
}

pub fn load_settings(path: &Path) -> Settings {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save_settings(path: &Path, settings: &Settings) {
    if let Ok(json) = serde_json::to_string_pretty(settings) {
        let _ = std::fs::write(path, json);
    }
}
