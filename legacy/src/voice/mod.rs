//! Bishop's voice: text-to-speech via ElevenLabs.
//!
//! `domain` holds the configuration (read from the environment); `client` is the
//! outbound HTTP call that turns a line of text into MP3 audio. The whole slice
//! is gated behind the `audio` feature, since playback lives in `crate::audio`.
pub mod client;
pub mod domain;
