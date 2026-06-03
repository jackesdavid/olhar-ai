//! The cold-open title card shown for ~3s before the boot self-test: a big
//! NOSTROMO banner with Weyland-Yutani branding, then it hands off to `boot`.
pub mod domain;
pub mod handler;
pub mod view;

pub use domain::SplashSequence;
