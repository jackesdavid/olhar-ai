//! Postgres adapter for the notification slice — owns the beacon + telegram-config
//! SQL (moved out of the shared `db.rs`). Functions take the shared [`Db`] infra
//! handle (pool + runtime).
use crate::db::Db;

use super::NotificationTarget;

/// The saved beacons, ordered by label. Empty (and logged) on a query error.
pub fn load_beacons(db: &Db) -> Vec<NotificationTarget> {
    let pool = db.pool();
    db.block(async move {
        sqlx::query_as::<_, (i64, String, bool)>(
            "SELECT chat_id, label, enabled FROM notification_targets ORDER BY label",
        )
        .fetch_all(&pool)
        .await
        .map(|rows| {
            rows.into_iter()
                .map(|(chat_id, label, enabled)| NotificationTarget { label, chat_id, enabled })
                .collect()
        })
        .unwrap_or_else(|e| {
            log::error!("notification::repo::load_beacons query failed: {e}");
            Vec::new()
        })
    })
}

/// Replace the whole beacon set (delete-all + insert each), mirroring the in-memory
/// store's "full list after every mutation" semantics. Best-effort.
pub fn save_beacons(db: &Db, beacons: &[NotificationTarget]) {
    let pool = db.pool();
    let beacons = beacons.to_vec();
    let res: Result<(), sqlx::Error> = db.block(async move {
        let mut tx = pool.begin().await?;
        sqlx::query("DELETE FROM notification_targets").execute(&mut *tx).await?;
        for b in &beacons {
            sqlx::query("INSERT INTO notification_targets (chat_id, label, enabled) VALUES ($1, $2, $3)")
                .bind(b.chat_id)
                .bind(&b.label)
                .bind(b.enabled)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(())
    });
    if let Err(e) = res {
        log::error!("notification::repo::save_beacons transaction failed: {e}");
    }
}

/// The saved Telegram (bot_token, pairing_code), or `None` if unset.
pub fn load_telegram(db: &Db) -> Option<(String, String)> {
    let pool = db.pool();
    db.block(async move {
        sqlx::query_as::<_, (String, String)>(
            "SELECT bot_token, pairing_code FROM telegram_config WHERE id = 1",
        )
        .fetch_optional(&pool)
        .await
        .unwrap_or_else(|e| {
            log::error!("notification::repo::load_telegram query failed: {e}");
            None
        })
    })
}

/// Upsert the single Telegram config row. Best-effort.
pub fn save_telegram(db: &Db, bot_token: &str, pairing_code: &str) {
    let pool = db.pool();
    let bot_token = bot_token.to_string();
    let pairing_code = pairing_code.to_string();
    let res: Result<(), sqlx::Error> = db.block(async move {
        sqlx::query(
            "INSERT INTO telegram_config (id, bot_token, pairing_code) VALUES (1, $1, $2) \
             ON CONFLICT (id) DO UPDATE SET bot_token = EXCLUDED.bot_token, \
             pairing_code = EXCLUDED.pairing_code",
        )
        .bind(&bot_token)
        .bind(&pairing_code)
        .execute(&pool)
        .await
        .map(|_| ())
    });
    if let Err(e) = res {
        log::error!("notification::repo::save_telegram failed: {e}");
    }
}
