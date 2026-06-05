//! Speech-to-text state and configuration. The recorder (microphone capture)
//! lives in `recorder` behind the `audio` feature; the transcription HTTP client
//! lives in `client`. This module holds the shared, feature-independent pieces:
//! the listening state the UI reads, the result event, and the Scribe config.
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use super::recorder::Recording;

/// Default ElevenLabs speech-to-text model.
pub const DEFAULT_STT_MODEL: &str = "scribe_v1";

/// A transcription result delivered from the background worker, drained each tick.
pub enum SttEvent {
    /// Speech recognized — route it to Bishop.
    Transcript(String),
    /// Capture/transcription failed; the reason is shown in the chat log.
    Failed(String),
}

/// Live "are we listening" state. `level` is a smoothed 0..1 microphone amplitude
/// the UI reads each frame to animate the listening waveform; `recording` holds
/// the active capture (present only while listening).
pub struct ListenState {
    pub active: bool,
    pub level: Arc<AtomicU32>,
    pub recording: Option<Recording>,
}

impl ListenState {
    pub fn new() -> Self {
        ListenState {
            active: false,
            level: Arc::new(AtomicU32::new(0)),
            recording: None,
        }
    }

    /// The current mic level (0.0..1.0) for the meter.
    pub fn level(&self) -> f32 {
        f32::from_bits(self.level.load(Ordering::Relaxed))
    }
}

impl Default for ListenState {
    fn default() -> Self {
        Self::new()
    }
}

/// Resolved Scribe request configuration, reusing the ElevenLabs key.
pub struct SttConfig {
    pub api_key: String,
    pub model_id: String,
}

impl SttConfig {
    /// Reads the configuration from the environment. Returns `None` when no API
    /// key is set — the signal callers use to report STT as unavailable.
    pub fn from_env() -> Option<Self> {
        let api_key = std::env::var("ELEVENLABS_API_KEY").ok()?;
        if api_key.trim().is_empty() {
            return None;
        }
        Some(SttConfig {
            api_key,
            model_id: std::env::var("ELEVENLABS_STT_MODEL")
                .unwrap_or_else(|_| DEFAULT_STT_MODEL.into()),
        })
    }
}
