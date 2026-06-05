//! Speech-to-text: talk to Bishop from any screen with F9.
//!
//! `domain` holds the listening state, the result event, and the Scribe config.
//! `recorder` captures the microphone (cpal, behind the `audio` feature) and
//! encodes a WAV. `client` POSTs that WAV to ElevenLabs Scribe and returns the
//! transcript. The app toggles capture on F9, then routes the transcript to
//! Bishop — whose spoken reply plays regardless of the current screen.
pub mod client;
pub mod domain;
pub mod recorder;

pub use domain::{ListenState, SttEvent};
