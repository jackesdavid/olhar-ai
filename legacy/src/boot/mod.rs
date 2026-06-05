//! The Nostromo-style boot sequence: a self-test scroll on the left and the
//! MU/TH/UR circuit-board graphic on the right, played once at startup.
pub mod domain;
pub mod handler;
pub mod view;

pub use domain::BootSequence;
