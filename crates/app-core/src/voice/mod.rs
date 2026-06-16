//! Bishop's voice: text-to-speech via ElevenLabs. Copied from the ratatui
//! `src/voice` for the web app. `domain` holds the env-resolved config; `client`
//! turns a line of text into MP3 bytes. Playback lives in the WebView (`<audio>`),
//! so the cpal player is not ported.
pub mod client;
pub mod domain;

pub use client::synthesize;
pub use domain::VoiceConfig;
