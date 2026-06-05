//! The device-detail feature: open a scanned camera to enter/validate
//! credentials and pull a live snapshot (rendered in monochrome phosphor).
pub mod actions;
pub mod domain;
pub mod handler;
pub mod view;

pub use domain::{DeviceDetail, Field};
