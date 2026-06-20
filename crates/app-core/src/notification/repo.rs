//! Postgres adapter for the notification slice — owns the beacon + telegram-config
//! SQL (moved out of the shared `db.rs`). Tenant-scoped: beacons keyed
//! `(tenant_id, chat_id)`; telegram config is one row per tenant.
//! The global Telegram provider config (shared bot token) lives in
//! `telegram_global_config` (single row, operator-managed).
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
/// Kept for callers that only need the raw per-tenant row (e.g. test-send, pairing
/// poller resolution before the global fallback is applied).
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

/// Tenant notification state for the Recipients page:
/// `(pairing_code, effective_bot_username, provider_configured)`.
/// `pairing_code` is empty if the tenant hasn't set one yet.
/// `effective_bot_username` falls back to the global provider's username.
/// `provider_configured` is true if there is a usable bot token (global or own).
pub fn load_tenant_notification_state(db: &Db, tenant_id: Uuid) -> (String, Option<String>, bool) {
    let tenant_row = {
        let pool = db.pool();
        db.block(async move {
            sqlx::query_as::<_, (String, String, Option<String>)>(
                "SELECT bot_token, pairing_code, bot_username FROM telegram_config WHERE tenant_id = $1",
            )
            .bind(tenant_id)
            .fetch_optional(&pool)
            .await
            .unwrap_or_else(|e| {
                log::error!("notification::repo::load_tenant_notification_state query failed: {e}");
                None
            })
        })
    };
    let global = load_global_telegram(db);
    let global_token_set = global.as_ref().map(|(t, _)| !t.trim().is_empty()).unwrap_or(false);
    let global_username = global.and_then(|(_, u)| u);

    match tenant_row {
        Some((own_token, pairing_code, own_username)) => {
            let provider_configured = !own_token.trim().is_empty() || global_token_set;
            let effective_username = own_username.or(global_username);
            (pairing_code, effective_username, provider_configured)
        }
        None => (String::new(), global_username, global_token_set),
    }
}

/// Upsert only the pairing_code for a tenant (bot_token left unchanged).
pub fn save_pairing_code(db: &Db, tenant_id: Uuid, pairing_code: &str) {
    let pool = db.pool();
    let pairing_code = pairing_code.to_string();
    let res: Result<(), sqlx::Error> = db.block(async move {
        sqlx::query(
            "INSERT INTO telegram_config (tenant_id, id, bot_token, pairing_code) \
             VALUES ($1, 1, '', $2) \
             ON CONFLICT (tenant_id) DO UPDATE SET pairing_code = EXCLUDED.pairing_code",
        )
        .bind(tenant_id)
        .bind(&pairing_code)
        .execute(&pool)
        .await
        .map(|_| ())
    });
    if let Err(e) = res {
        log::error!("notification::repo::save_pairing_code failed: {e}");
    }
}

/// Load the global Telegram provider config: `(bot_token, bot_username)`.
/// Returns `None` if the row is missing (table not yet migrated).
pub fn load_global_telegram(db: &Db) -> Option<(String, Option<String>)> {
    let pool = db.pool();
    db.block(async move {
        sqlx::query_as::<_, (String, Option<String>)>(
            "SELECT bot_token, bot_username FROM telegram_global_config WHERE id = 1",
        )
        .fetch_optional(&pool)
        .await
        .unwrap_or_else(|e| {
            log::error!("notification::repo::load_global_telegram query failed: {e}");
            None
        })
    })
}

/// Upsert the global bot token. Best-effort.
pub fn save_global_telegram(db: &Db, bot_token: &str) {
    let pool = db.pool();
    let bot_token = bot_token.to_string();
    let res: Result<(), sqlx::Error> = db.block(async move {
        sqlx::query(
            "INSERT INTO telegram_global_config (id, bot_token) VALUES (1, $1) \
             ON CONFLICT (id) DO UPDATE SET bot_token = EXCLUDED.bot_token",
        )
        .bind(&bot_token)
        .execute(&pool)
        .await
        .map(|_| ())
    });
    if let Err(e) = res {
        log::error!("notification::repo::save_global_telegram failed: {e}");
    }
}

/// Persist the resolved global bot username. Best-effort.
pub fn save_global_telegram_username(db: &Db, username: &str) {
    let pool = db.pool();
    let username = username.to_string();
    let res: Result<(), sqlx::Error> = db.block(async move {
        sqlx::query("UPDATE telegram_global_config SET bot_username = $1 WHERE id = 1")
            .bind(&username)
            .execute(&pool)
            .await
            .map(|_| ())
    });
    if let Err(e) = res {
        log::error!("notification::repo::save_global_telegram_username failed: {e}");
    }
}

/// All tenants that have a pairing code set, merged with the global bot token as
/// fallback. Used by sentinel-flow to bootstrap one pairing poller per configured tenant.
pub fn load_all_telegram(db: &Db) -> Vec<(Uuid, super::NotificationConfig)> {
    // Effective global token: global config row, else env var fallback.
    let global_token = load_global_telegram(db)
        .map(|(t, _)| t)
        .filter(|t| !t.trim().is_empty())
        .or_else(|| super::NotificationConfig::from_env().map(|c| c.bot_token))
        .unwrap_or_default();

    let pool = db.pool();
    let rows: Vec<(Uuid, String, String)> = db.block(async move {
        sqlx::query_as::<_, (Uuid, String, String)>(
            "SELECT tenant_id, COALESCE(bot_token, ''), COALESCE(pairing_code, '') \
             FROM telegram_config \
             WHERE pairing_code IS NOT NULL AND pairing_code <> ''",
        )
        .fetch_all(&pool)
        .await
        .unwrap_or_else(|e| {
            log::error!("notification::repo::load_all_telegram query failed: {e}");
            Vec::new()
        })
    });

    rows.into_iter()
        .filter_map(|(tenant, own_token, pairing_code)| {
            let effective_token =
                if own_token.trim().is_empty() { global_token.clone() } else { own_token };
            if effective_token.trim().is_empty() {
                return None; // no bot token anywhere — skip this tenant
            }
            Some((tenant, super::NotificationConfig { bot_token: effective_token, pairing_code }))
        })
        .collect()
}

/// Upsert the tenant's full Telegram config row (token + pairing code). Best-effort.
/// Still used by the old per-tenant bot flow and by test-send.
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

/// Persist a resolved per-tenant bot username. Best-effort.
pub fn save_telegram_username(db: &Db, tenant_id: Uuid, username: &str) {
    let pool = db.pool();
    let username = username.to_string();
    let res: Result<(), sqlx::Error> = db.block(async move {
        sqlx::query("UPDATE telegram_config SET bot_username = $1 WHERE tenant_id = $2")
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
