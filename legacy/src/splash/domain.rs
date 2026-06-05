//! Splash-card state and the big NOSTROMO banner it shows. Behaviour lives in
//! [`super::handler`].

/// How long the splash holds before handing off to the boot scroll. 60 ticks at
/// the app's 50ms cadence ≈ 3 seconds.
pub const SPLASH_TOTAL_TICKS: u32 = 60;

/// Tracks how long the splash card has been on screen.
pub struct SplashSequence {
    pub elapsed: u32,
}

impl SplashSequence {
    pub fn new() -> Self {
        SplashSequence { elapsed: 0 }
    }
}

impl Default for SplashSequence {
    fn default() -> Self {
        Self::new()
    }
}
