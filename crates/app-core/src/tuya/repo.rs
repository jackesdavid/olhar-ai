//! Postgres adapter for the `tuya_devices` table (per-tenant). Same shape as
//! `devices::repo`: takes the shared [`Db`] handle + `tenant_id`, runs its own SQL via
//! `Db::block`.
use uuid::Uuid;

use crate::db::Db;
use crate::timeutil::now_rfc3339;

use super::domain::TuyaDevice;

const COLS: &str = "device_id, name, category, product_id, online, functions_json";

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
