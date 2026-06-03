//! Alert actions shared by the dashboard UI and the Bishop tools: create,
//! deliver (dispatch to beacons + record), and delete. Delivery does its network
//! IO inline, so callers run it on a background thread.
use crate::notification::{NotificationConfig, NotificationStore, NotificationTarget};

use super::domain::{next_id, now_rfc3339, save_alerts, Alert, AlertLevel, AlertStore, Delivery};

/// The single line sent to a beacon for an alert: `[WARN] front garage — <msg>`.
pub fn format_message(a: &Alert) -> String {
    let loc = a.room_name.as_deref().unwrap_or("ship");
    format!("[{}] {} \u{2014} {}", a.level.label(), loc, a.message)
}

/// Append a new alert to the store and persist. Returns the created alert.
pub fn create(
    store: &AlertStore,
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
    save_alerts(&alerts);
    alert
}

/// Remove the alert with `id` and persist. Returns true if one was removed.
pub fn delete(store: &AlertStore, id: u64) -> bool {
    let mut alerts = store.lock().unwrap();
    let before = alerts.len();
    alerts.retain(|a| a.id != id);
    let removed = alerts.len() != before;
    if removed {
        save_alerts(&alerts);
    }
    removed
}

/// Deliver the alert with `id` to every enabled beacon, recording a [`Delivery`]
/// per beacon, and persist. Returns a human status string. Network IO is inline.
pub fn deliver(store: &AlertStore, notif: &NotificationStore, id: u64) -> Result<String, String> {
    let cfg = NotificationConfig::from_env()
        .ok_or("Notifications are not configured (set TELEGRAM_BOT_TOKEN).")?;

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
        let ok = crate::notification::dispatch(notif, &cfg, Some(&b.label), &msg, None).is_ok();
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
        save_alerts(&alerts);
    }
    Ok(format!("Delivered alert {id} to {ok_count}/{} beacon(s).", beacons.len()))
}
