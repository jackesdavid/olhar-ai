//! Notification data: the trusted target list (persisted to `notifications.json`)
//! and the Telegram configuration read from the environment. Mirrors the
//! `scan` device store and the `voice`/`agent` `from_env` config pattern.
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

/// The shared list of trusted notification targets — held by `App` and cloned
/// into the pairing poller and the agent thread, so the `send_notification`
/// tool reads the same store the poller writes.
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

fn targets_path() -> PathBuf {
    PathBuf::from("notifications.json")
}

pub fn load_targets() -> Vec<NotificationTarget> {
    std::fs::read_to_string(targets_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save_targets(targets: &[NotificationTarget]) {
    if let Ok(json) = serde_json::to_string_pretty(targets) {
        let _ = std::fs::write(targets_path(), json);
    }
}

/// Resolved Telegram configuration. `from_env` returns `None` when the bot token
/// is unset — the signal callers use to report notifications as unavailable (and
/// to skip spawning the pairing poller).
///
///   TELEGRAM_BOT_TOKEN    — required; from @BotFather
///   TELEGRAM_PAIRING_CODE — required for pairing; the secret users send the bot
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
