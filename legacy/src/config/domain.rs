//! Ship Configurations: the user-tunable settings, persisted to `config.json`
//! next to `devices.json`. These drive the audio mixer (via
//! [`AudioLevels`](crate::audio::levels::AudioLevels)), the chat teletype speed,
//! and the speech-to-text behaviour — replacing values that used to be hardcoded.
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Smallest / largest chars-revealed-per-tick for the chat teletype.
pub const TYPING_MIN: u8 = 1;
pub const TYPING_MAX: u8 = 8;

/// All live settings. Levels are 0.0..=1.0 channel gains that multiply each
/// sound's inherent mix; defaults of 1.0 reproduce the original loudness exactly.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub master: f32,
    pub voice: f32,
    pub ui: f32,
    pub ambient: f32,
    pub voice_enabled: bool,
    pub typing_speed: u8,
    pub stt_autosend: bool,
    /// RTSP transport for live camera feeds. `#[serde(default)]` so older
    /// `config.json` files written before this field still load (→ TCP).
    #[serde(default)]
    pub transport: Transport,
    /// Whether the CRT window paints the slow top-to-bottom refresh roll (the
    /// Alien-monitor sweep). GPU-window only; ignored on the `--tty` frontend.
    /// `default = "default_true"` so older configs load with it enabled.
    #[serde(default = "default_true")]
    pub crt_sweep: bool,
    /// Refresh-roll strength, 0.0..=1.0; scaled into peak brightness on the GPU.
    /// `default = "default_sweep_intensity"` so older configs get a sane value.
    #[serde(default = "default_sweep_intensity")]
    pub crt_sweep_intensity: f32,
}

/// serde default for `crt_sweep` — on unless a saved config says otherwise.
fn default_true() -> bool {
    true
}

/// serde default for `crt_sweep_intensity` — a gentle half-strength roll.
fn default_sweep_intensity() -> f32 {
    0.5
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
            crt_sweep: true,
            crt_sweep_intensity: 0.5,
        }
    }
}

/// Which lower-layer transport ffmpeg uses to pull an RTSP stream. TCP is the
/// reliable default (no packet-loss artifacts); UDP trades reliability for lower
/// latency and rides through loss with glitches instead of stalls; Auto lets
/// ffmpeg negotiate (UDP first, then fall back to TCP).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Transport {
    #[default]
    Tcp,
    Udp,
    Auto,
}

impl Transport {
    /// In selector order, the values the CONFIG row cycles through.
    pub const ALL: [Transport; 3] = [Transport::Tcp, Transport::Udp, Transport::Auto];

    pub fn label(self) -> &'static str {
        match self {
            Transport::Tcp => "TCP",
            Transport::Udp => "UDP",
            Transport::Auto => "AUTO",
        }
    }

    /// Cycle by a +1 / -1 nudge with wraparound — used by the CONFIG handler.
    pub fn cycled(self, delta: i8) -> Transport {
        let i = Self::ALL.iter().position(|t| *t == self).unwrap_or(0) as i8;
        let n = Self::ALL.len() as i8;
        Self::ALL[(i + delta).rem_euclid(n) as usize]
    }
}

/// One editable row on the configurations screen, in display order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Master,
    Voice,
    Ui,
    Ambient,
    VoiceEnabled,
    TypingSpeed,
    SttAutosend,
    RtspTransport,
    CrtSweep,
    CrtSweepIntensity,
}

impl Field {
    /// The rows, top to bottom — the order the cursor walks.
    pub const ALL: [Field; 10] = [
        Field::Master,
        Field::Voice,
        Field::Ui,
        Field::Ambient,
        Field::VoiceEnabled,
        Field::TypingSpeed,
        Field::SttAutosend,
        Field::RtspTransport,
        Field::CrtSweep,
        Field::CrtSweepIntensity,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Field::Master => "MASTER VOLUME",
            Field::Voice => "BISHOP VOICE",
            Field::Ui => "UI / KEYS",
            Field::Ambient => "AMBIENT / HUM",
            Field::VoiceEnabled => "VOICE OUTPUT",
            Field::TypingSpeed => "TELETYPE SPEED",
            Field::SttAutosend => "SPEECH AUTO-SEND",
            Field::RtspTransport => "RTSP TRANSPORT",
            Field::CrtSweep => "CRT REFRESH ROLL",
            Field::CrtSweepIntensity => "ROLL INTENSITY",
        }
    }
}

fn settings_path() -> PathBuf {
    PathBuf::from("config.json")
}

pub fn load_settings() -> Settings {
    std::fs::read_to_string(settings_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save_settings(settings: &Settings) {
    if let Ok(json) = serde_json::to_string_pretty(settings) {
        let _ = std::fs::write(settings_path(), json);
    }
}
