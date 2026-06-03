//! Cursor + editing logic for the Ship Configurations screen. The screen owns
//! only which row is focused; the actual values live in [`Settings`] so they can
//! be persisted and shared with the audio mixer.
use super::domain::{Field, Settings, TYPING_MAX, TYPING_MIN};

/// How much one Left/Right press nudges a 0..1 level.
const STEP: f32 = 0.05;

/// Transient UI state for the configurations screen: just the focused row.
pub struct ConfigScreen {
    pub focus: Field,
}

impl ConfigScreen {
    pub fn new() -> Self {
        ConfigScreen { focus: Field::Master }
    }

    fn index(&self) -> usize {
        Field::ALL.iter().position(|f| *f == self.focus).unwrap_or(0)
    }

    pub fn focus_next(&mut self) {
        let i = (self.index() + 1) % Field::ALL.len();
        self.focus = Field::ALL[i];
    }

    pub fn focus_prev(&mut self) {
        let n = Field::ALL.len();
        let i = (self.index() + n - 1) % n;
        self.focus = Field::ALL[i];
    }

    /// Apply a +1 / -1 nudge to the focused field. Returns `true` if the change
    /// affects audio (so the caller can re-sync the live mixer).
    pub fn adjust(&self, settings: &mut Settings, delta: i8) -> bool {
        let d = delta as f32 * STEP;
        match self.focus {
            Field::Master => {
                settings.master = (settings.master + d).clamp(0.0, 1.0);
                true
            }
            Field::Voice => {
                settings.voice = (settings.voice + d).clamp(0.0, 1.0);
                true
            }
            Field::Ui => {
                settings.ui = (settings.ui + d).clamp(0.0, 1.0);
                true
            }
            Field::Ambient => {
                settings.ambient = (settings.ambient + d).clamp(0.0, 1.0);
                true
            }
            Field::VoiceEnabled => {
                if delta != 0 {
                    settings.voice_enabled = !settings.voice_enabled;
                }
                true
            }
            Field::TypingSpeed => {
                let next = settings.typing_speed as i16 + delta as i16;
                settings.typing_speed = next.clamp(TYPING_MIN as i16, TYPING_MAX as i16) as u8;
                false
            }
            Field::SttAutosend => {
                if delta != 0 {
                    settings.stt_autosend = !settings.stt_autosend;
                }
                false
            }
            Field::RtspTransport => {
                if delta != 0 {
                    settings.transport = settings.transport.cycled(delta);
                }
                false
            }
            Field::CrtSweep => {
                if delta != 0 {
                    settings.crt_sweep = !settings.crt_sweep;
                }
                false
            }
            Field::CrtSweepIntensity => {
                settings.crt_sweep_intensity =
                    (settings.crt_sweep_intensity + d).clamp(0.0, 1.0);
                false
            }
        }
    }
}

impl Default for ConfigScreen {
    fn default() -> Self {
        Self::new()
    }
}
