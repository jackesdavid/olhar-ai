//! `app-core` — Bishop's UI-agnostic logic, copied from the ratatui `src/` for the web app.
//!
//! The ratatui app (`/src`) is FROZEN; this is an independent copy so it's never touched
//! (see `docs/web-migration-plan.md`). Each migration unit copies a module's `domain`/
//! `actions` here verbatim and the Tauri shell wraps them as commands.
//!
//! Path-parameterized persistence (no hardcoded cwd files) so the web app writes its own
//! state under the OS app-data dir and never clobbers the ratatui app's JSON files.

pub mod agent;
pub mod alerts;
pub mod platform_logs;
pub mod config;
pub mod db;
pub mod device_actions;
pub mod devices;
pub mod go2rtc;
pub mod listen;
pub mod map;
pub mod media;
pub mod notification;
pub mod onvif;
pub mod persons;
pub mod ports;
pub mod presence;
pub mod tenancy;
pub mod voice;
pub mod workflows;
