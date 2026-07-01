//! The notification feature — copied from the ratatui `src/notification/`.
//!
//! Send Bishop alerts (text, or text + a camera snapshot) to trusted mobile
//! targets over Telegram. Targets are added via a secure `/pair <code>` flow and
//! persisted to `notifications.json`.
//!
//! `domain` holds the [`NotificationTarget`] store + path-parameterized
//! load/save and the [`NotificationConfig`]. `client` is the Telegram Bot API
//! (sendMessage / sendPhoto / getUpdates). `handler` runs the pairing poller, the
//! core [`dispatch`] send entry point, and the beacon management helpers.
//!
//! The ratatui `view`/`BeaconsScreen` UI state is DROPPED — the web app drives
//! the Beacons screen from Svelte.
pub mod client;
pub mod domain;
pub mod handler;
pub mod queue;
pub mod repo;

pub use domain::{load_targets, NotificationConfig, NotificationStore, NotificationTarget};
pub use handler::{
    delete_target, dispatch, rename_target, set_enabled_by_id, spawn_pairing,
};
