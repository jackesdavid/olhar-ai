//! Tuya smart-home integration — the operator's single Tuya Cloud project (credentials in
//! env, like `ANTHROPIC_API_KEY`), a per-tenant catalog of the devices its linked app
//! accounts expose, and a signed HTTP client to list/read/control them.
//!
//! - [`domain`] — [`TuyaConfig`] (`from_env`), the credential-free [`TuyaDevice`] row, and
//!   [`TuyaFunction`] (the type metadata the editor uses to render a value widget).
//! - [`client`] — HMAC-SHA256 signed calls to the Tuya Cloud (token, list, status, command).
//! - [`repo`] — the `tuya_devices` table (per-tenant), populated by the sync route.
//!
//! Shared by `sentinel-control` (list/sync routes) and `sentinel-flow` (the workflow nodes).
pub mod client;
pub mod domain;
pub mod repo;

pub use domain::{TuyaConfig, TuyaDevice, TuyaFunction};
