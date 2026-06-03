//! Ship Configurations: live, persisted settings (volume mixer, voice toggle,
//! teletype speed, speech auto-send) and the screen for editing them.
//!
//! `domain` holds the [`Settings`] data + `config.json` load/save. `handler`
//! owns the on-screen cursor and value editing. `view` renders the knobs.
pub mod domain;
pub mod handler;
pub mod view;

pub use domain::{load_settings, save_settings, Settings};
pub use handler::ConfigScreen;
