//! Speech-to-text configuration. The mic capture + UI state from the ratatui
//! `src/listen/domain.rs` (`ListenState`/`SttEvent`/recorder) are dropped — the
//! WebView owns capture. This keeps only the env-resolved Scribe config.
//!
//!   ELEVENLABS_API_KEY   - required; reuses the TTS key
//!   ELEVENLABS_STT_MODEL - optional; defaults to scribe_v1

/// Default ElevenLabs speech-to-text model.
pub const DEFAULT_STT_MODEL: &str = "scribe_v1";

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
