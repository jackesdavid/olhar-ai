//! Postgres adapter for the `tuya_devices` table (per-tenant). Same shape as
//! `devices::repo`: takes the shared [`Db`] handle + `tenant_id`, runs its own SQL via
//! `Db::block`.
use uuid::Uuid;

use crate::db::Db;
use crate::timeutil::now_rfc3339;

use super::domain::{TuyaConfig, TuyaDevice};

const COLS: &str = "device_id, name, category, product_id, online, functions_json, function_labels_json";

/// The tenant's linked Tuya devices, ordered by name. Empty (and logged) on a query error.
pub fn load(db: &Db, tenant_id: Uuid) -> Vec<TuyaDevice> {
    let pool = db.pool();
    db.block(async move {
        let sql = format!("SELECT {COLS} FROM tuya_devices WHERE tenant_id = $1 ORDER BY name");
        sqlx::query_as::<_, TuyaDevice>(&sql).bind(tenant_id).fetch_all(&pool).await.unwrap_or_else(|e| {
            log::error!("tuya::repo::load query failed: {e}");
            Vec::new()
        })
    })
}

/// Resolve a selector (the stable `device_id`) to a device row for this tenant.
pub fn find_selector(db: &Db, tenant_id: Uuid, selector: &str) -> Option<TuyaDevice> {
    let sel = selector.trim().to_string();
    if sel.is_empty() {
        return None;
    }
    let pool = db.pool();
    db.block(async move {
        let sql = format!("SELECT {COLS} FROM tuya_devices WHERE tenant_id = $1 AND device_id = $2");
        sqlx::query_as::<_, TuyaDevice>(&sql).bind(tenant_id).bind(&sel).fetch_optional(&pool).await.unwrap_or_else(|e| {
            log::error!("tuya::repo::find_selector query failed: {e}");
            None
        })
    })
}

/// Upsert a synced device set for the tenant (insert or update by `(tenant_id, device_id)`).
/// Rows the tenant no longer has are left in place — a full-replace would drop a device that
/// was briefly offline during sync; unlinking is a separate, explicit action (future work).
pub fn upsert_many(db: &Db, tenant_id: Uuid, devices: &[TuyaDevice]) -> Result<(), String> {
    let pool = db.pool();
    let devices = devices.to_vec();
    let now = now_rfc3339();
    let res: Result<(), sqlx::Error> = db.block(async move {
        let mut tx = pool.begin().await?;
        for d in &devices {
            sqlx::query(
                "INSERT INTO tuya_devices \
                   (tenant_id, device_id, name, category, product_id, online, functions_json, updated_at) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8) \
                 ON CONFLICT (tenant_id, device_id) DO UPDATE SET \
                   name = EXCLUDED.name, category = EXCLUDED.category, product_id = EXCLUDED.product_id, \
                   online = EXCLUDED.online, functions_json = EXCLUDED.functions_json, updated_at = EXCLUDED.updated_at",
            )
            .bind(tenant_id)
            .bind(&d.device_id)
            .bind(&d.name)
            .bind(&d.category)
            .bind(&d.product_id)
            .bind(d.online)
            .bind(&d.functions_json)
            .bind(&now)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    });
    res.map_err(|e| {
        log::error!("tuya::repo::upsert_many failed: {e}");
        format!("database error: {e}")
    })
}

/// Set a device's friendly per-function labels (`{ "<code>": "<label>" }` JSON) — a targeted
/// update that survives sync (the column is not in `upsert_many`'s ON CONFLICT set).
pub fn set_function_labels(db: &Db, tenant_id: Uuid, device_id: &str, labels_json: &str) -> Result<(), String> {
    let pool = db.pool();
    let device_id = device_id.to_string();
    let labels_json = labels_json.to_string();
    let res: Result<u64, sqlx::Error> = db.block(async move {
        let r = sqlx::query("UPDATE tuya_devices SET function_labels_json = $3 WHERE tenant_id = $1 AND device_id = $2")
            .bind(tenant_id)
            .bind(&device_id)
            .bind(&labels_json)
            .execute(&pool)
            .await?;
        Ok(r.rows_affected())
    });
    match res {
        Ok(0) => Err("device not found".to_string()),
        Ok(_) => Ok(()),
        Err(e) => {
            log::error!("tuya::repo::set_function_labels failed: {e}");
            Err(format!("database error: {e}"))
        }
    }
}

/// Record a linked Tuya app account for the tenant (OAuth). Idempotent by `(tenant_id, tuya_uid)`.
pub fn link_account(db: &Db, tenant_id: Uuid, tuya_uid: &str) -> Result<(), String> {
    let pool = db.pool();
    let uid = tuya_uid.to_string();
    let now = now_rfc3339();
    let res: Result<(), sqlx::Error> = db.block(async move {
        sqlx::query(
            "INSERT INTO tuya_account_links (tenant_id, tuya_uid, linked_at) VALUES ($1, $2, $3) \
             ON CONFLICT (tenant_id, tuya_uid) DO UPDATE SET linked_at = EXCLUDED.linked_at",
        )
        .bind(tenant_id)
        .bind(&uid)
        .bind(&now)
        .execute(&pool)
        .await?;
        Ok(())
    });
    res.map_err(|e| {
        log::error!("tuya::repo::link_account failed: {e}");
        format!("database error: {e}")
    })
}

/// The effective Tuya credentials for a tenant: their own saved config ("bring your own project"),
/// else the operator env fallback (`TuyaConfig::from_env`). `None` when neither is set.
pub fn resolve_config(db: &Db, tenant_id: Uuid) -> Option<TuyaConfig> {
    load_config(db, tenant_id).or_else(TuyaConfig::from_env)
}

/// The tenant's own saved Tuya credentials (the `tuya_config` row), if any.
pub fn load_config(db: &Db, tenant_id: Uuid) -> Option<TuyaConfig> {
    let pool = db.pool();
    db.block(async move {
        let row: Option<(String, String, String)> =
            sqlx::query_as("SELECT access_id, access_key, endpoint FROM tuya_config WHERE tenant_id = $1")
                .bind(tenant_id)
                .fetch_optional(&pool)
                .await
                .unwrap_or_else(|e| {
                    log::error!("tuya::repo::load_config failed: {e}");
                    None
                });
        row.map(|(access_id, access_key, endpoint)| TuyaConfig { access_id, access_key, endpoint })
    })
}

/// Save (upsert) the tenant's own Tuya credentials.
pub fn save_config(db: &Db, tenant_id: Uuid, cfg: &TuyaConfig) -> Result<(), String> {
    let pool = db.pool();
    let cfg = cfg.clone();
    let now = now_rfc3339();
    let res: Result<(), sqlx::Error> = db.block(async move {
        sqlx::query(
            "INSERT INTO tuya_config (tenant_id, access_id, access_key, endpoint, updated_at) \
             VALUES ($1, $2, $3, $4, $5) \
             ON CONFLICT (tenant_id) DO UPDATE SET access_id = EXCLUDED.access_id, \
               access_key = EXCLUDED.access_key, endpoint = EXCLUDED.endpoint, updated_at = EXCLUDED.updated_at",
        )
        .bind(tenant_id)
        .bind(&cfg.access_id)
        .bind(&cfg.access_key)
        .bind(&cfg.endpoint)
        .bind(&now)
        .execute(&pool)
        .await?;
        Ok(())
    });
    res.map_err(|e| {
        log::error!("tuya::repo::save_config failed: {e}");
        format!("database error: {e}")
    })
}

/// Remove the tenant's own Tuya credentials (disconnect → falls back to operator env, if any).
pub fn delete_config(db: &Db, tenant_id: Uuid) -> Result<(), String> {
    let pool = db.pool();
    let res: Result<(), sqlx::Error> = db.block(async move {
        sqlx::query("DELETE FROM tuya_config WHERE tenant_id = $1").bind(tenant_id).execute(&pool).await?;
        Ok(())
    });
    res.map_err(|e| {
        log::error!("tuya::repo::delete_config failed: {e}");
        format!("database error: {e}")
    })
}

/// The Tuya uid(s) this tenant has linked. Empty = not linked yet.
pub fn linked_uids(db: &Db, tenant_id: Uuid) -> Vec<String> {
    let pool = db.pool();
    db.block(async move {
        sqlx::query_scalar::<_, String>("SELECT tuya_uid FROM tuya_account_links WHERE tenant_id = $1")
            .bind(tenant_id)
            .fetch_all(&pool)
            .await
            .unwrap_or_else(|e| {
                log::error!("tuya::repo::linked_uids query failed: {e}");
                Vec::new()
            })
    })
}
