//! ONVIF camera support: WS-Discovery, credential validation, and JPEG
//! snapshots — ported from the cornea agent to bishop's blocking model
//! (`std::net::UdpSocket` + `ureq`, no async runtime). Network calls are run on
//! background threads by the caller.
pub mod client;
pub mod discovery;
pub mod domain;
mod xml;

pub use domain::DiscoveredDevice;
