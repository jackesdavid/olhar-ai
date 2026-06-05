//! Alert data: the persisted alert log (`alerts.json`) plus the level enum and
//! the pure stats helpers the dashboard and tools share. Mirrors the
//! `notification`/`map`/`scan` JSON-store pattern.
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

/// The shared alert log — held by `App` and cloned into the agent thread so the
/// alert tools read/write the same store the dashboard renders.
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
    /// In ascending-severity order — the order the CREATE selector cycles.
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

    /// Parse a level name (case-insensitive); used by the Bishop tools.
    pub fn parse(s: &str) -> Option<AlertLevel> {
        match s.trim().to_ascii_lowercase().as_str() {
            "debug" => Some(AlertLevel::Debug),
            "info" => Some(AlertLevel::Info),
            "warn" | "warning" => Some(AlertLevel::Warn),
            "error" | "err" => Some(AlertLevel::Error),
            _ => None,
        }
    }

    /// Cycle by a +1 / -1 nudge with wraparound — used by the CREATE selector.
    pub fn cycled(self, delta: i8) -> AlertLevel {
        let i = Self::ALL.iter().position(|l| *l == self).unwrap_or(0) as i8;
        let n = Self::ALL.len() as i8;
        Self::ALL[(i + delta).rem_euclid(n) as usize]
    }
}

impl Default for AlertLevel {
    fn default() -> Self {
        AlertLevel::Info
    }
}

/// One delivery of an alert to a beacon: which target, when, and whether it
/// went through.
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

fn alerts_path() -> PathBuf {
    PathBuf::from("alerts.json")
}

pub fn load_alerts() -> Vec<Alert> {
    std::fs::read_to_string(alerts_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save_alerts(alerts: &[Alert]) {
    if let Ok(json) = serde_json::to_string_pretty(alerts) {
        let _ = std::fs::write(alerts_path(), json);
    }
}

/// Next monotonic id for a new alert.
pub fn next_id(alerts: &[Alert]) -> u64 {
    alerts.iter().map(|a| a.id).max().map_or(1, |m| m + 1)
}

// ---- pure stats helpers (shared by the dashboard + tools) ------------------

/// Indices of the alerts raised today, newest first.
pub fn today_indices(alerts: &[Alert]) -> Vec<usize> {
    let mut idx: Vec<usize> =
        (0..alerts.len()).filter(|&i| is_today(&alerts[i].created_at)).collect();
    idx.sort_by(|&a, &b| alerts[b].created_at.cmp(&alerts[a].created_at));
    idx
}

/// Count of today's alerts per level, indexed like [`AlertLevel::ALL`].
pub fn counts_by_level(alerts: &[Alert]) -> [usize; 4] {
    let mut out = [0usize; 4];
    for &i in &today_indices(alerts) {
        let pos = AlertLevel::ALL.iter().position(|l| *l == alerts[i].level).unwrap_or(0);
        out[pos] += 1;
    }
    out
}

/// Today's room-tagged alert counts, as `(room name, count)` sorted desc.
pub fn counts_by_room(alerts: &[Alert]) -> Vec<(String, usize)> {
    use std::collections::HashMap;
    let mut map: HashMap<String, usize> = HashMap::new();
    for &i in &today_indices(alerts) {
        if let Some(name) = &alerts[i].room_name {
            *map.entry(name.clone()).or_insert(0) += 1;
        }
    }
    let mut v: Vec<(String, usize)> = map.into_iter().collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    v
}

/// How many of today's alerts have not been delivered to any beacon.
pub fn pending_today(alerts: &[Alert]) -> usize {
    today_indices(alerts).iter().filter(|&&i| !alerts[i].delivered()).count()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alert(id: u64, level: AlertLevel, room: Option<&str>, when: &str, delivered: bool) -> Alert {
        Alert {
            id,
            level,
            message: "m".into(),
            room_id: room.map(|_| 0),
            room_name: room.map(|s| s.into()),
            source: "test".into(),
            created_at: when.into(),
            deliveries: if delivered {
                vec![Delivery { target: "x".into(), chat_id: 1, at: when.into(), ok: true }]
            } else {
                vec![]
            },
        }
    }

    #[test]
    fn next_id_is_monotonic() {
        assert_eq!(next_id(&[]), 1);
        let a = vec![alert(3, AlertLevel::Info, None, "t", false)];
        assert_eq!(next_id(&a), 4);
    }

    #[test]
    fn level_parse_and_cycle() {
        assert_eq!(AlertLevel::parse("WARN"), Some(AlertLevel::Warn));
        assert_eq!(AlertLevel::parse("warning"), Some(AlertLevel::Warn));
        assert_eq!(AlertLevel::parse("nope"), None);
        assert_eq!(AlertLevel::Debug.cycled(-1), AlertLevel::Error); // wraps
        assert_eq!(AlertLevel::Error.cycled(1), AlertLevel::Debug);
    }

    #[test]
    fn today_stats_filter_and_group() {
        let today = now_rfc3339();
        let alerts = vec![
            alert(1, AlertLevel::Warn, Some("garage"), &today, false),
            alert(2, AlertLevel::Warn, Some("garage"), &today, true),
            alert(3, AlertLevel::Error, None, &today, false),
            alert(4, AlertLevel::Info, Some("hall"), "2000-01-01T00:00:00+00:00", false), // not today
        ];
        assert_eq!(today_indices(&alerts).len(), 3);
        let by_level = counts_by_level(&alerts);
        assert_eq!(by_level[AlertLevel::ALL.iter().position(|l| *l == AlertLevel::Warn).unwrap()], 2);
        let by_room = counts_by_room(&alerts);
        assert_eq!(by_room[0], ("garage".to_string(), 2));
        assert_eq!(pending_today(&alerts), 2); // alert 1 + alert 3 undelivered today
    }
}
