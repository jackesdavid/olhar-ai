//! Postgres adapter for the notification slice — owns the beacon + telegram-config
//! SQL (moved out of the shared `db.rs`). Tenant-scoped: beacons keyed
//! `(tenant_id, chat_id)`; telegram config is one row per tenant.
use uuid::Uuid;

use crate::db::Db;

use super::NotificationTarget;

/// The tenant's beacons, ordered by label. Empty (and logged) on a query error.
pub fn load_beacons(db: &Db, tenant_id: Uuid) -> Vec<NotificationTarget> {
    let pool = db.pool();
    db.block(async move {
        sqlx::query_as::<_, (i64, String, bool)>(
            "SELECT chat_id, label, enabled FROM notification_targets WHERE tenant_id = $1 ORDER BY label",
        )
        .bind(tenant_id)
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

/// Replace the tenant's beacon set (delete-all-for-tenant + insert each). Best-effort.
pub fn save_beacons(db: &Db, tenant_id: Uuid, beacons: &[NotificationTarget]) {
    let pool = db.pool();
    let beacons = beacons.to_vec();
    let res: Result<(), sqlx::Error> = db.block(async move {
        let mut tx = pool.begin().await?;
        sqlx::query("DELETE FROM notification_targets WHERE tenant_id = $1").bind(tenant_id).execute(&mut *tx).await?;
        for b in &beacons {
            sqlx::query("INSERT INTO notification_targets (tenant_id, chat_id, label, enabled) VALUES ($1, $2, $3, $4)")
                .bind(tenant_id)
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

/// The tenant's saved Telegram (bot_token, pairing_code), or `None` if unset.
pub fn load_telegram(db: &Db, tenant_id: Uuid) -> Option<(String, String)> {
    let pool = db.pool();
    db.block(async move {
        sqlx::query_as::<_, (String, String)>(
            "SELECT bot_token, pairing_code FROM telegram_config WHERE tenant_id = $1",
        )
        .bind(tenant_id)
        .fetch_optional(&pool)
        .await
        .unwrap_or_else(|e| {
            log::error!("notification::repo::load_telegram query failed: {e}");
            None
        })
    })
}

/// The tenant's full Telegram config including the resolved bot username, or
/// `None` if unset. Returns `(bot_token, pairing_code, bot_username)`.
pub fn load_telegram_full(db: &Db, tenant_id: Uuid) -> Option<(String, String, Option<String>)> {
    let pool = db.pool();
    db.block(async move {
        sqlx::query_as::<_, (String, String, Option<String>)>(
            "SELECT bot_token, pairing_code, bot_username FROM telegram_config WHERE tenant_id = $1",
        )
        .bind(tenant_id)
        .fetch_optional(&pool)
        .await
        .unwrap_or_else(|e| {
            log::error!("notification::repo::load_telegram_full query failed: {e}");
            None
        })
    })
}

/// Persist a resolved bot username back to the config row. Best-effort.
pub fn save_telegram_username(db: &Db, tenant_id: Uuid, username: &str) {
    let pool = db.pool();
    let username = username.to_string();
    let res: Result<(), sqlx::Error> = db.block(async move {
        sqlx::query(
            "UPDATE telegram_config SET bot_username = $1 WHERE tenant_id = $2",
        )
        .bind(&username)
        .bind(tenant_id)
        .execute(&pool)
        .await
        .map(|_| ())
    });
    if let Err(e) = res {
        log::error!("notification::repo::save_telegram_username failed: {e}");
    }
}

/// All tenants that have a bot token set, as `(tenant_id, NotificationConfig)`.
/// Used by sentinel-flow to bootstrap one pairing poller per configured tenant.
pub fn load_all_telegram(db: &Db) -> Vec<(Uuid, super::NotificationConfig)> {
    let pool = db.pool();
    db.block(async move {
        sqlx::query_as::<_, (Uuid, String, String)>(
            "SELECT tenant_id, bot_token, pairing_code FROM telegram_config \
             WHERE bot_token IS NOT NULL AND bot_token <> ''",
        )
        .fetch_all(&pool)
        .await
        .map(|rows| {
            rows.into_iter()
                .map(|(t, bot_token, pairing_code)| (t, super::NotificationConfig { bot_token, pairing_code }))
                .collect()
        })
        .unwrap_or_else(|e| {
            log::error!("notification::repo::load_all_telegram query failed: {e}");
            Vec::new()
        })
    })
}

/// Upsert the tenant's Telegram config row. Best-effort.
pub fn save_telegram(db: &Db, tenant_id: Uuid, bot_token: &str, pairing_code: &str) {
    let pool = db.pool();
    let bot_token = bot_token.to_string();
    let pairing_code = pairing_code.to_string();
    let res: Result<(), sqlx::Error> = db.block(async move {
        sqlx::query(
            "INSERT INTO telegram_config (tenant_id, id, bot_token, pairing_code) VALUES ($1, 1, $2, $3) \
             ON CONFLICT (tenant_id) DO UPDATE SET bot_token = EXCLUDED.bot_token, \
             pairing_code = EXCLUDED.pairing_code",
        )
        .bind(tenant_id)
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
