//! The "scan devices" feature: an Aliens-style motion tracker that sweeps the
//! local volume and reports range/bearing on every contact it resolves.
pub mod domain;
pub mod handler;
pub mod view;

pub use domain::{Device, DeviceStore, ScanState};
