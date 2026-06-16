//! Speech-to-text via ElevenLabs Scribe. Copied from the ratatui `src/listen`
//! for the web app, minus the cpal `recorder` and the ratatui `ListenState` —
//! the WebView captures the microphone (`MediaRecorder`) and sends the audio
//! bytes here. `domain` holds the env-resolved config; `client` is the Scribe
//! transcription call.
pub mod client;
pub mod domain;

pub use client::transcribe;
pub use domain::SttConfig;
