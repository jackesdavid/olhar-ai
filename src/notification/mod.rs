//! The notification feature: send Bishop alerts (text, or text + a camera
//! snapshot) to trusted mobile targets over Telegram. Targets are added via a
//! secure `/pair <code>` flow and persisted to `notifications.json`.
//!
//! `domain` holds the [`NotificationTarget`] store + `notifications.json`
//! load/save and the env [`NotificationConfig`]. `client` is the Telegram Bot
//! API (sendMessage / sendPhoto / getUpdates). `handler` runs the pairing poller
//! and the provider- and UI-agnostic core [`dispatch`] — the single send entry
//! point the Bishop tool uses today and the future workflows engine will call
//! directly.
pub mod client;
pub mod domain;
pub mod handler;
pub mod view;

pub use domain::{load_targets, NotificationConfig, NotificationStore};
pub use handler::{dispatch, spawn_pairing, AlertsScreen};
