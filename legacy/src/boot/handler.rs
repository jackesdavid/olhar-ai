//! Boot-sequence logic: advancing the scroll and firing its self-test sounds.
use crate::audio::Sfx;

use super::domain::{BootSequence, BOOT_REVEAL_TICKS, BOOT_SCRIPT, BOOT_TOTAL_TICKS};

impl BootSequence {
    /// Advance the boot scroll one tick. Lines are revealed proportionally over
    /// the first `BOOT_REVEAL_TICKS` (beeping on each non-blank line, chiming on
    /// the last); the screen then holds on READY. Returns `true` exactly once —
    /// when the full `BOOT_TOTAL_TICKS` window elapses and the app should hand
    /// off to the menu.
    pub fn tick(&mut self, sfx: &dyn Sfx) -> bool {
        self.elapsed += 1;
        let len = BOOT_SCRIPT.len();

        if self.revealed < len {
            // How many lines should be visible by now, spread across the window.
            let want =
                ((self.elapsed as u64 * len as u64) / BOOT_REVEAL_TICKS as u64) as usize;
            let want = want.min(len);
            while self.revealed < want {
                // Beep on non-blank lines for that chattering self-test feel.
                if !BOOT_SCRIPT[self.revealed].is_empty() {
                    sfx.beep();
                }
                self.revealed += 1;
                if self.revealed == len {
                    self.done_at = Some(self.elapsed);
                    sfx.ready();
                }
            }
        }

        self.revealed >= len && self.elapsed >= BOOT_TOTAL_TICKS
    }

    /// Skip the animation entirely (any keypress during boot): reveal everything
    /// and fire the ready chime. The caller performs the handoff.
    pub fn skip(&mut self, sfx: &dyn Sfx) {
        self.revealed = BOOT_SCRIPT.len();
        self.elapsed = BOOT_TOTAL_TICKS;
        self.done_at = Some(self.elapsed);
        sfx.ready();
    }
}
