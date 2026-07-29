//! TTS configuration. Resolved from the environment so the voice can be enabled
//! or swapped without a recompile. Copied verbatim from the ratatui
//! `src/voice/domain.rs`.
//!
//!   ELEVENLABS_API_KEY   - required; without it the voice is disabled
//!   ELEVENLABS_VOICE_ID  - optional; defaults to a deep, calm male voice
//!   ELEVENLABS_MODEL_ID  - optional; defaults to eleven_multilingual_v2

// "Adam" — a deep, measured male voice that suits Bishop. Override via
// ELEVENLABS_VOICE_ID to use a custom / cloned voice.
pub const DEFAULT_VOICE_ID: &str = "pNInz6obpgDQGcFmaJgB";
pub const DEFAULT_MODEL_ID: &str = "eleven_multilingual_v2";

// Voice tuning sent with every request.
pub const STABILITY: f32 = 0.45;
pub const SIMILARITY_BOOST: f32 = 0.8;

/// A fully-resolved request configuration. Construct it with [`VoiceConfig::from_env`].
pub struct VoiceConfig {
    pub api_key: String,
    pub voice_id: String,
    pub model_id: String,
}

impl VoiceConfig {
    /// Reads the configuration from the environment. Returns `None` when no API
    /// key is set, which is the signal callers use to stay silent.
    pub fn from_env() -> Option<Self> {
        let api_key = std::env::var("ELEVENLABS_API_KEY").ok()?;
        if api_key.trim().is_empty() {
            return None;
        }
        Some(Self {
            api_key,
            voice_id: std::env::var("ELEVENLABS_VOICE_ID")
                .unwrap_or_else(|_| DEFAULT_VOICE_ID.into()),
            model_id: std::env::var("ELEVENLABS_MODEL_ID")
                .unwrap_or_else(|_| DEFAULT_MODEL_ID.into()),
        })
    }
}
