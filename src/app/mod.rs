//! The application root: aggregate state, the lifecycle/run loop, and the
//! top-level view that composes the individual slices.
pub mod domain;
pub mod handler;
pub mod view;

pub use domain::App;
