//! Alert data + actions, copied from the ratatui `src/alerts/{domain,actions}.rs`.
//!
//! The ratatui app (`/src`) is FROZEN; this is an independent copy. As with the
//! other copied modules, persistence is path-parameterized (no hardcoded cwd
//! `alerts.json`) so the Tauri shell writes under the OS app-data dir.
//!
//! `deliver` (which dispatches to beacons via the notification layer) was
//! deferred to migration unit 017 and lands here, ported from the ratatui
//! `src/alerts/actions.rs::deliver`. Persistence stays path-parameterized.
use std::path::Path;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use crate::notification::{self, NotificationConfig, NotificationStore, NotificationTarget};

/// The shared alert log — held by the Tauri managed state and cloned into
/// background threads so commands read/mutate the same store.
pub type AlertStore = Arc<Mutex<Vec<Alert>>>;

/// Severity of an alert, low → high.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AlertLevel {
    Debug,
    Info,
    Warn,
    Error,
}

impl AlertLevel {
    /// In ascending-severity order.
    pub const ALL: [AlertLevel; 4] =
        [AlertLevel::Debug, AlertLevel::Info, AlertLevel::Warn, AlertLevel::Error];

    pub fn label(self) -> &'static str {
        match self {
            AlertLevel::Debug => "DEBUG",
            AlertLevel::Info => "INFO",
            AlertLevel::Warn => "WARN",
            AlertLevel::Error => "ERROR",
        }
    }

    /// Parse a level name (case-insensitive).
    pub fn parse(s: &str) -> Option<AlertLevel> {
        match s.trim().to_ascii_lowercase().as_str() {
            "debug" => Some(AlertLevel::Debug),
            "info" => Some(AlertLevel::Info),
            "warn" | "warning" => Some(AlertLevel::Warn),
            "error" | "err" => Some(AlertLevel::Error),
            _ => None,
        }
    }
}

impl Default for AlertLevel {
    fn default() -> Self {
        AlertLevel::Info
    }
}

/// One delivery of an alert to a beacon: which target, when, and whether it
/// went through. Kept as data only here (delivery logic lands in unit 017).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Delivery {
    pub target: String,
    pub chat_id: i64,
    pub at: String, // RFC3339
    pub ok: bool,
}

/// A single alert record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Alert {
    pub id: u64,
    pub level: AlertLevel,
    pub message: String,
    /// Position: the map room this alert belongs to, if any (optional).
    #[serde(default)]
    pub room_id: Option<u32>,
    /// The room's name at creation time, kept so display survives a rename/delete.
    #[serde(default)]
    pub room_name: Option<String>,
    /// The subsystem that raised the alert ("operator", "bishop", "scanner"…).
    pub source: String,
    pub created_at: String, // RFC3339
    /// Who received this alert and when (the delivered status).
    #[serde(default)]
    pub deliveries: Vec<Delivery>,
}

impl Alert {
    /// Whether at least one delivery succeeded.
    pub fn delivered(&self) -> bool {
        self.deliveries.iter().any(|d| d.ok)
    }
}

/// Current local time as an RFC3339 string (the stored timestamp format).
pub fn now_rfc3339() -> String {
    chrono::Local::now().to_rfc3339()
}

/// Whether an RFC3339 timestamp falls on today's local date.
pub fn is_today(created_at: &str) -> bool {
    let today = chrono::Local::now().date_naive();
    chrono::DateTime::parse_from_rfc3339(created_at)
        .map(|dt| dt.with_timezone(&chrono::Local).date_naive() == today)
        .unwrap_or(false)
}

/// Load the saved alert log from `path`. Returns an empty list if the file is
/// missing or unreadable.
pub fn load_alerts(path: &Path) -> Vec<Alert> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// Persist the alert log to `path` (pretty JSON). Best-effort: write errors are
/// swallowed, matching the ratatui app's behavior.
pub fn save_alerts(path: &Path, alerts: &[Alert]) {
    if let Ok(json) = serde_json::to_string_pretty(alerts) {
        let _ = std::fs::write(path, json);
    }
}

/// Next monotonic id for a new alert.
pub fn next_id(alerts: &[Alert]) -> u64 {
    alerts.iter().map(|a| a.id).max().map_or(1, |m| m + 1)
}

/// Append a new alert to the store and persist to `path`. Returns the created
/// alert.
pub fn create(
    store: &AlertStore,
    path: &Path,
    level: AlertLevel,
    message: String,
    room_id: Option<u32>,
    room_name: Option<String>,
    source: String,
) -> Alert {
    let mut alerts = store.lock().unwrap();
    let alert = Alert {
        id: next_id(&alerts),
        level,
        message,
        room_id,
        room_name,
        source,
        created_at: now_rfc3339(),
        deliveries: Vec::new(),
    };
    alerts.push(alert.clone());
    save_alerts(path, &alerts);
    alert
}

/// Remove the alert with `id` and persist to `path`. Returns true if one was
/// removed.
pub fn delete(store: &AlertStore, path: &Path, id: u64) -> bool {
    let mut alerts = store.lock().unwrap();
    let before = alerts.len();
    alerts.retain(|a| a.id != id);
    let removed = alerts.len() != before;
    if removed {
        save_alerts(path, &alerts);
    }
    removed
}

/// The single line sent to a beacon for an alert: `[WARN] front garage — <msg>`.
pub fn format_message(a: &Alert) -> String {
    let loc = a.room_name.as_deref().unwrap_or("ship");
    format!("[{}] {} \u{2014} {}", a.level.label(), loc, a.message)
}

/// Deliver the alert with `id` to every enabled beacon, recording a [`Delivery`]
/// per beacon, persist to `path`, and return a human status string. Network IO is
/// inline, so callers run it on a background thread. Ported from the ratatui
/// `src/alerts/actions.rs::deliver` (the config is passed in rather than read
/// from env here, so the Tauri shell can source it from a saved file).
pub fn deliver(
    store: &AlertStore,
    path: &Path,
    notif: &NotificationStore,
    cfg: &NotificationConfig,
    id: u64,
) -> Result<String, String> {
    // Snapshot the message + the enabled beacons under their locks, release first.
    let msg = {
        let alerts = store.lock().unwrap();
        let a = alerts
            .iter()
            .find(|a| a.id == id)
            .ok_or_else(|| format!("Alert {id} not found."))?;
        format_message(a)
    };
    let beacons: Vec<NotificationTarget> = {
        let targets = notif.lock().unwrap();
        targets.iter().filter(|t| t.enabled).cloned().collect()
    };
    if beacons.is_empty() {
        return Err("No live beacons to deliver to.".into());
    }

    let mut deliveries = Vec::new();
    let mut ok_count = 0usize;
    for b in &beacons {
        let ok = notification::dispatch(notif, cfg, Some(&b.label), &msg, None).is_ok();
        if ok {
            ok_count += 1;
        }
        deliveries.push(Delivery {
            target: b.label.clone(),
            chat_id: b.chat_id,
            at: now_rfc3339(),
            ok,
        });
    }

    {
        let mut alerts = store.lock().unwrap();
        if let Some(a) = alerts.iter_mut().find(|a| a.id == id) {
            a.deliveries.extend(deliveries);
        }
        save_alerts(path, &alerts);
    }
    Ok(format!("Delivered alert {id} to {ok_count}/{} beacon(s).", beacons.len()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk(id: u64, level: AlertLevel) -> Alert {
        Alert {
            id,
            level,
            message: "m".into(),
            room_id: None,
            room_name: None,
            source: "test".into(),
            created_at: now_rfc3339(),
            deliveries: vec![],
        }
    }

    #[test]
    fn next_id_is_monotonic() {
        assert_eq!(next_id(&[]), 1);
        assert_eq!(next_id(&[mk(3, AlertLevel::Info)]), 4);
    }

    #[test]
    fn level_parse() {
        assert_eq!(AlertLevel::parse("WARN"), Some(AlertLevel::Warn));
        assert_eq!(AlertLevel::parse("warning"), Some(AlertLevel::Warn));
        assert_eq!(AlertLevel::parse("err"), Some(AlertLevel::Error));
        assert_eq!(AlertLevel::parse("nope"), None);
    }

    #[test]
    fn create_delete_round_trip_through_path() {
        let dir = std::env::temp_dir().join("app_core_alerts_test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("alerts.json");
        let _ = std::fs::remove_file(&path);

        let store: AlertStore = Arc::new(Mutex::new(load_alerts(&path)));
        let a = create(
            &store,
            &path,
            AlertLevel::Warn,
            "hello".into(),
            None,
            Some("garage".into()),
            "operator".into(),
        );
        assert_eq!(a.id, 1);
        assert_eq!(load_alerts(&path).len(), 1);
        assert!(delete(&store, &path, a.id));
        assert!(!delete(&store, &path, a.id));
        assert!(load_alerts(&path).is_empty());
        let _ = std::fs::remove_file(&path);
    }
}
