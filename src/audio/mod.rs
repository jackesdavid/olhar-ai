//! Audio for the boot sequence and UI.
//!
//! `domain` defines the [`Sfx`] port the rest of the app talks to (plus a silent
//! fallback and the [`init`] factory). `adapter` is the real rodio-backed device
//! that implements it — compiled in only with the `audio` feature. If there is
//! no audio device, no `assets/` directory, or the feature is off, every call
//! becomes a cheap no-op and the app runs silently.
//!
//! Drop your WAV/OGG files in `assets/` next to the binary (or in the project
//! root when running via `cargo run`):
//!   assets/boot_loop.wav   - the long "burst of blinks and noise" hum
//!   assets/beep.wav        - short blip, fired as each boot line prints
//!   assets/ready.wav       - the chime when INTERFACE 2037 comes online
//!
//! Get license-clean files from Pixabay / itch.io retro packs, or generate them
//! with a tool like 8-Bit Beep Box. Do NOT ship the original film audio.
#[cfg(feature = "audio")]
pub mod adapter;
pub mod domain;
pub mod levels;

pub use domain::{init, Sfx};
pub use levels::AudioLevels;
