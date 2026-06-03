//! Live, shared volume controls. One `Arc<AudioLevels>` is created at startup
//! from the saved [`Settings`](crate::config::Settings) and handed to the audio
//! backend; the Ship Configurations screen mutates the same atomics, so volume
//! and mute changes take effect immediately with no restart. The values are the
//! single source of truth that replaces the old hardcoded `amplify` constants.
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

/// Atomic, thread-safe mixer levels (each channel 0.0..=1.0) plus a voice mute.
pub struct AudioLevels {
    master: AtomicU32,
    voice: AtomicU32,
    ui: AtomicU32,
    ambient: AtomicU32,
    voice_enabled: AtomicBool,
}

impl AudioLevels {
    pub fn new(master: f32, voice: f32, ui: f32, ambient: f32, voice_enabled: bool) -> Self {
        AudioLevels {
            master: AtomicU32::new(master.to_bits()),
            voice: AtomicU32::new(voice.to_bits()),
            ui: AtomicU32::new(ui.to_bits()),
            ambient: AtomicU32::new(ambient.to_bits()),
            voice_enabled: AtomicBool::new(voice_enabled),
        }
    }

    // The gain getters below are read only by the rodio adapter, so they go
    // unused (along with this helper) in `--no-default-features` builds.
    #[cfg_attr(not(feature = "audio"), allow(dead_code))]
    fn get(slot: &AtomicU32) -> f32 {
        f32::from_bits(slot.load(Ordering::Relaxed))
    }
    fn set(slot: &AtomicU32, v: f32) {
        slot.store(v.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
    }

    pub fn set_master(&self, v: f32) {
        Self::set(&self.master, v);
    }
    pub fn set_voice(&self, v: f32) {
        Self::set(&self.voice, v);
    }
    pub fn set_ui(&self, v: f32) {
        Self::set(&self.ui, v);
    }
    pub fn set_ambient(&self, v: f32) {
        Self::set(&self.ambient, v);
    }
    pub fn set_voice_enabled(&self, on: bool) {
        self.voice_enabled.store(on, Ordering::Relaxed);
    }

    #[cfg_attr(not(feature = "audio"), allow(dead_code))]
    pub fn voice_enabled(&self) -> bool {
        self.voice_enabled.load(Ordering::Relaxed)
    }

    /// Final gain for a UI sound (keystrokes, beeps): `ui x master`.
    #[cfg_attr(not(feature = "audio"), allow(dead_code))]
    pub fn ui_gain(&self) -> f32 {
        Self::get(&self.ui) * Self::get(&self.master)
    }
    /// Final gain for ambient sounds (boot hum, working loop): `ambient x master`.
    #[cfg_attr(not(feature = "audio"), allow(dead_code))]
    pub fn ambient_gain(&self) -> f32 {
        Self::get(&self.ambient) * Self::get(&self.master)
    }
    /// Final gain for Bishop's voice (TTS): `voice x master`.
    #[cfg_attr(not(feature = "audio"), allow(dead_code))]
    pub fn voice_gain(&self) -> f32 {
        Self::get(&self.voice) * Self::get(&self.master)
    }
}
