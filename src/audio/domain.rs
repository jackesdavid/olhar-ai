//! The audio port and its silent fallback.
use std::sync::Arc;

use super::levels::AudioLevels;

/// The public surface the rest of the app talks to. Identical signatures whether
/// or not the `audio` feature is compiled in.
pub trait Sfx {
    fn boot_loop(&self);
    fn beep(&self);
    fn ready(&self);
    fn hush(&self);
    /// Play a single key-click, cycling through the `keyNN.wav` samples. No-op
    /// when silent.
    fn keystroke(&self) {}
    /// Like [`keystroke`](Self::keystroke) but quieter — used for Bishop's
    /// typewriter so it doesn't compete with his voice.
    fn type_tick(&self) {}
    /// Keep a looping "working" sound playing while `active` (cycling through the
    /// `working_NN.wav` samples), and stop it otherwise. Called every tick.
    fn set_working(&self, _active: bool) {}
    /// Speak a line in Bishop's voice via ElevenLabs TTS. The network call and
    /// playback happen on a background thread so the render loop never blocks.
    /// `on_ready` is invoked the moment playback begins — or immediately if TTS
    /// isn't configured / fails — so the caller can reveal the text in sync with
    /// the audio. The default (silent) backend reveals it right away.
    fn speak(&self, _text: &str, on_ready: Box<dyn FnOnce() + Send>) {
        on_ready();
    }
    /// Whether any sound (boot hum, beep, chime, or voice) is currently playing.
    /// Drives the audio indicator in the top bar. Always `false` when silent.
    fn is_active(&self) -> bool {
        false
    }
}

/// Builds the best available sound backend, wired to the shared mixer `levels`
/// so volume/mute changes take effect live. Returns a boxed trait object so
/// callers don't care which one they got.
pub fn init(levels: Arc<AudioLevels>) -> Box<dyn Sfx> {
    #[cfg(feature = "audio")]
    {
        if let Some(a) = super::adapter::Audio::new(levels.clone()) {
            return Box::new(a);
        }
        // else fall through to silence
    }
    let _ = levels; // silent backend ignores the mixer
    Box::new(Silent)
}

/// Fallback backend: does nothing, costs nothing. Relies on the trait's default
/// `speak` no-op.
pub struct Silent;
impl Sfx for Silent {
    fn boot_loop(&self) {}
    fn beep(&self) {}
    fn ready(&self) {}
    fn hush(&self) {}
}
