//! Notification data — copied from the ratatui `src/notification/domain.rs`.
//!
//! The ratatui app (`/src`) is FROZEN; this is an independent copy. Persistence
//! is path-parameterized (no hardcoded cwd `notifications.json`) so the Tauri
//! shell writes under the OS app-data dir.
use std::path::Path;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

/// The shared list of trusted notification targets — held by the Tauri managed
/// state and cloned into the pairing poller, so the send commands read the same
/// store the poller writes.
pub type NotificationStore = Arc<Mutex<Vec<NotificationTarget>>>;

/// A trusted destination for Bishop's alerts. A target becomes trusted only by
/// sending the pairing code to the bot; persisted to `notifications.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationTarget {
    /// Human label, taken from the Telegram sender's first name / username at
    /// pairing time. Used to address a send ("notify my phone").
    pub label: String,
    /// Telegram chat id to deliver to.
    pub chat_id: i64,
    /// Whether this target receives alerts. A paired target is live by default;
    /// `default_true` keeps an older/hand-edited record without the field on.
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

/// Load the saved target list from `path`. Returns an empty list if the file is
/// missing or unreadable.
pub fn load_targets(path: &Path) -> Vec<NotificationTarget> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// Persist the target list to `path` (pretty JSON). Best-effort: write errors are
/// swallowed, matching the ratatui app's behavior.
pub fn save_targets(path: &Path, targets: &[NotificationTarget]) {
    if let Ok(json) = serde_json::to_string_pretty(targets) {
        let _ = std::fs::write(path, json);
    }
}

/// Resolved Telegram configuration. `from_env` returns `None` when the bot token
/// is unset — the signal callers use to report notifications as unavailable (and
/// to skip spawning the pairing poller).
///
///   TELEGRAM_BOT_TOKEN    — required; from @BotFather
///   TELEGRAM_PAIRING_CODE — required for pairing; the secret users send the bot
#[derive(Debug, Clone)]
pub struct NotificationConfig {
    pub bot_token: String,
    pub pairing_code: String,
}

impl NotificationConfig {
    pub fn from_env() -> Option<Self> {
        let bot_token = std::env::var("TELEGRAM_BOT_TOKEN").ok()?;
        if bot_token.trim().is_empty() {
            return None;
        }
        let pairing_code = std::env::var("TELEGRAM_PAIRING_CODE").unwrap_or_default();
        Some(Self { bot_token, pairing_code })
    }

    /// Whether inbound pairing can run (needs a non-empty pairing code). The
    /// token alone is enough to *send*, so this only gates the poller.
    pub fn can_pair(&self) -> bool {
        !self.pairing_code.trim().is_empty()
    }
}
