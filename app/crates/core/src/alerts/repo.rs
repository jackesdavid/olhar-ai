//! Postgres adapter for the alerts slice — the persistence port for the alert log.
//! Owns the alert SQL (moved out of the shared `db.rs`); the domain (`Alert`) stays
//! pure. Functions take the shared [`Db`] infra handle (pool + runtime).
use crate::db::Db;

use super::{Alert, AlertLevel, Delivery};

/// The saved alert log, oldest id first (matching the in-memory append order).
/// Empty (and logged) on a query error — best-effort, like the devices repo.
pub fn load(db: &Db) -> Vec<Alert> {
    let pool = db.pool();
    db.block(async move {
        type Row = (i64, String, String, Option<i64>, Option<String>, String, String, serde_json::Value);
        let rows = sqlx::query_as::<_, Row>(
            "SELECT id, level, message, room_id, room_name, source, created_at, deliveries \
             FROM alerts ORDER BY id",
        )
        .fetch_all(&pool)
        .await;
        match rows {
            Ok(rows) => rows
                .into_iter()
                .map(|(id, level, message, room_id, room_name, source, created_at, deliveries)| Alert {
                    id: id as u64,
                    level: AlertLevel::parse(&level).unwrap_or_default(),
                    message,
                    room_id: room_id.map(|r| r as u32),
                    room_name,
                    source,
                    created_at,
                    deliveries: serde_json::from_value::<Vec<Delivery>>(deliveries).unwrap_or_default(),
                })
                .collect(),
            Err(e) => {
                log::error!("alerts::repo::load query failed: {e}");
                Vec::new()
            }
        }
    })
}

/// Insert one alert row (id is app-assigned). Best-effort; errors are logged.
pub fn insert(db: &Db, a: &Alert) {
    let pool = db.pool();
    let id = a.id as i64;
    let level = a.level.label().to_ascii_lowercase();
    let message = a.message.clone();
    let room_id = a.room_id.map(|r| r as i64);
    let room_name = a.room_name.clone();
    let source = a.source.clone();
    let created_at = a.created_at.clone();
    let deliveries = serde_json::to_value(&a.deliveries).unwrap_or_else(|_| serde_json::json!([]));
    let res: Result<(), sqlx::Error> = db.block(async move {
        sqlx::query(
            "INSERT INTO alerts (id, level, message, room_id, room_name, source, created_at, deliveries) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8) ON CONFLICT (id) DO NOTHING",
        )
        .bind(id)
        .bind(&level)
        .bind(&message)
        .bind(room_id)
        .bind(&room_name)
        .bind(&source)
        .bind(&created_at)
        .bind(&deliveries)
        .execute(&pool)
        .await
        .map(|_| ())
    });
    if let Err(e) = res {
        log::error!("alerts::repo::insert failed: {e}");
    }
}

/// Delete one alert row by id. Best-effort.
pub fn delete(db: &Db, id: u64) {
    let pool = db.pool();
    let id = id as i64;
    let res: Result<(), sqlx::Error> = db.block(async move {
        sqlx::query("DELETE FROM alerts WHERE id = $1").bind(id).execute(&pool).await.map(|_| ())
    });
    if let Err(e) = res {
        log::error!("alerts::repo::delete failed: {e}");
    }
}

/// Replace an alert's delivery list (after a deliver run). Best-effort.
pub fn update_deliveries(db: &Db, id: u64, deliveries: &[Delivery]) {
    let pool = db.pool();
    let id = id as i64;
    let json = serde_json::to_value(deliveries).unwrap_or_else(|_| serde_json::json!([]));
    let res: Result<(), sqlx::Error> = db.block(async move {
        sqlx::query("UPDATE alerts SET deliveries = $2 WHERE id = $1")
            .bind(id)
            .bind(&json)
            .execute(&pool)
            .await
            .map(|_| ())
    });
    if let Err(e) = res {
        log::error!("alerts::repo::update_deliveries failed: {e}");
    }
}

/// Read a legacy `alerts.json` (empty if missing/unreadable) — for the one-time
/// JSON→Postgres import on first DB boot only.
pub fn import_legacy_json(path: &std::path::Path) -> Vec<Alert> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}
