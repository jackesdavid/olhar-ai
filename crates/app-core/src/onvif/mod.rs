//! ONVIF camera support: WS-Discovery, credential validation, and JPEG
//! snapshots — copied verbatim from the ratatui `src/onvif` (blocking model:
//! `std::net::UdpSocket` + `ureq`, no async runtime). Network calls are run on
//! background threads by the caller (Tauri `spawn_blocking`).
pub mod client;
pub mod discovery;
pub mod domain;
mod xml;

pub use domain::DiscoveredDevice;
