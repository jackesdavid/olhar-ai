//! Postgres adapter for the devices slice — owns the device SQL (moved out of the
//! shared `db.rs`). Functions take the shared [`Db`] infra handle (pool + runtime)
//! and the `tenant_id` to scope to (rows are per-tenant; PK is `(tenant_id, ip)`).
use uuid::Uuid;

use crate::db::Db;

use super::Device;

const COLS: &str = "id, ip, xaddr, name, manufacturer, model, username, password, rtsp_url, \
     stream_mode, probe_status, probe_detail, probe_at, last_frame_at";

/// The tenant's device list, ordered by name. Empty (and logged) on a query error.
pub fn load(db: &Db, tenant_id: Uuid) -> Vec<Device> {
    let pool = db.pool();
    db.block(async move {
        let sql = format!("SELECT {COLS} FROM devices WHERE tenant_id = $1 ORDER BY name");
        sqlx::query_as::<_, Device>(&sql)
            .bind(tenant_id)
            .fetch_all(&pool)
            .await
            .unwrap_or_else(|e| {
                log::error!("devices::repo::load query failed: {e}");
                Vec::new()
            })
    })
}

/// Replace the tenant's device set in one transaction (delete-all-for-tenant + insert
/// each). The in-memory store is authoritative after each mutation. Best-effort.
pub fn save(db: &Db, tenant_id: Uuid, devices: &[Device]) {
    let pool = db.pool();
    let devices = devices.to_vec();
    let res: Result<(), sqlx::Error> = db.block(async move {
        let mut tx = pool.begin().await?;
        sqlx::query("DELETE FROM devices WHERE tenant_id = $1").bind(tenant_id).execute(&mut *tx).await?;
        for d in &devices {
            sqlx::query(
                // Status columns are carried through deliberately: this is a delete-all +
                // re-insert, so omitting them would wipe every camera's status on any device edit.
                "INSERT INTO devices \
                 (tenant_id, ip, id, xaddr, name, manufacturer, model, username, password, rtsp_url, stream_mode, \
                  probe_status, probe_detail, probe_at, last_frame_at) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)",
            )
            .bind(tenant_id)
            .bind(&d.ip)
            .bind(&d.id)
            .bind(&d.xaddr)
            .bind(&d.name)
            .bind(&d.manufacturer)
            .bind(&d.model)
            .bind(&d.username)
            .bind(&d.password)
            .bind(&d.rtsp_url)
            .bind(&d.stream_mode)
            .bind(&d.probe_status)
            .bind(&d.probe_detail)
            .bind(&d.probe_at)
            .bind(&d.last_frame_at)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    });
    if let Err(e) = res {
        log::error!("devices::repo::save transaction failed: {e}");
    }
}

/// Reconcile a camera that changed IP (matched by its stable ONVIF `id`): repoint its row
/// at the new ip/xaddr and carry the `configured_cameras` flag over. If a different row
/// already occupies `new_ip`, it's dropped first (the moved camera wins the address).
/// Returns true if a device row was updated. Best-effort.
pub fn move_ip(
    db: &Db,
    tenant_id: Uuid,
    onvif_id: &str,
    old_ip: &str,
    new_ip: &str,
    xaddr: &str,
) -> bool {
    if onvif_id.trim().is_empty() || new_ip.trim().is_empty() {
        return false;
    }
    let pool = db.pool();
    let onvif_id = onvif_id.to_string();
    let old_ip = old_ip.to_string();
    let new_ip = new_ip.to_string();
    let xaddr = xaddr.to_string();
    let res: Result<bool, sqlx::Error> = db.block(async move {
        let mut tx = pool.begin().await?;
        // Free the destination ip if a different camera holds it.
        sqlx::query("DELETE FROM devices WHERE tenant_id = $1 AND ip = $2 AND id <> $3")
            .bind(tenant_id)
            .bind(&new_ip)
            .bind(&onvif_id)
            .execute(&mut *tx)
            .await?;
        let r =
            sqlx::query("UPDATE devices SET ip = $2, xaddr = $3 WHERE tenant_id = $1 AND id = $4")
                .bind(tenant_id)
                .bind(&new_ip)
                .bind(&xaddr)
                .bind(&onvif_id)
                .execute(&mut *tx)
                .await?;
        // Keep the "configured" flag pointing at the new ip (table lives in the same DB).
        sqlx::query("UPDATE configured_cameras SET ip = $2 WHERE tenant_id = $1 AND ip = $3")
            .bind(tenant_id)
            .bind(&new_ip)
            .bind(&old_ip)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(r.rows_affected() > 0)
    });
    res.unwrap_or_else(|e| {
        log::error!("devices::repo::move_ip failed: {e}");
        false
    })
}
