//! Postgres adapter for the devices slice — owns the device SQL (moved out of the
//! shared `db.rs`). Functions take the shared [`Db`] infra handle (pool + runtime).
use crate::db::Db;

use super::Device;

const COLS: &str = "id, ip, xaddr, name, manufacturer, model, username, password, rtsp_url";

/// The saved device list, ordered by name. Empty (and logged) on a query error.
pub fn load(db: &Db) -> Vec<Device> {
    let pool = db.pool();
    db.block(async move {
        let sql = format!("SELECT {COLS} FROM devices ORDER BY name");
        sqlx::query_as::<_, Device>(&sql).fetch_all(&pool).await.unwrap_or_else(|e| {
            log::error!("devices::repo::load query failed: {e}");
            Vec::new()
        })
    })
}

/// Replace the whole device set in one transaction (delete-all + insert each). The
/// in-memory store is authoritative after each mutation, so replace-all mirrors the
/// old "write the whole file" semantics. Best-effort.
pub fn save(db: &Db, devices: &[Device]) {
    let pool = db.pool();
    let devices = devices.to_vec();
    let res: Result<(), sqlx::Error> = db.block(async move {
        let mut tx = pool.begin().await?;
        sqlx::query("DELETE FROM devices").execute(&mut *tx).await?;
        for d in &devices {
            sqlx::query(
                "INSERT INTO devices \
                 (ip, id, xaddr, name, manufacturer, model, username, password, rtsp_url) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
            )
            .bind(&d.ip)
            .bind(&d.id)
            .bind(&d.xaddr)
            .bind(&d.name)
            .bind(&d.manufacturer)
            .bind(&d.model)
            .bind(&d.username)
            .bind(&d.password)
            .bind(&d.rtsp_url)
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
