//! Splash-card logic: just a timer. The boot hum already starts in `App::new`,
//! so it plays under the splash with no extra work here.
use super::domain::{SplashSequence, SPLASH_TOTAL_TICKS};

impl SplashSequence {
    /// Advance the splash one tick. Returns `true` once the ~3s window elapses
    /// and the app should hand off to the boot scroll.
    pub fn tick(&mut self) -> bool {
        self.elapsed += 1;
        self.elapsed >= SPLASH_TOTAL_TICKS
    }
}
