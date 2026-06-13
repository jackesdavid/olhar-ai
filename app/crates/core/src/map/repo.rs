//! Postgres adapter for the map slice — owns the floor-plan SQL (moved out of the
//! shared `db.rs`). Functions take the shared [`Db`] infra handle (pool + runtime).
use crate::db::Db;

use super::{normalize_rooms, MapRoom};

/// The saved floor-plan rooms, ordered by id, with camera offsets normalized.
/// Malformed rows are skipped. Empty on a query error.
pub fn load(db: &Db) -> Vec<MapRoom> {
    let pool = db.pool();
    db.block(async move {
        let rows = sqlx::query_as::<_, (serde_json::Value,)>("SELECT data FROM map_rooms ORDER BY id")
            .fetch_all(&pool)
            .await;
        let mut rooms: Vec<MapRoom> = match rows {
            Ok(rows) => rows.into_iter().filter_map(|(v,)| serde_json::from_value::<MapRoom>(v).ok()).collect(),
            Err(e) => {
                log::error!("map::repo::load query failed: {e}");
                Vec::new()
            }
        };
        normalize_rooms(&mut rooms);
        rooms
    })
}

/// Replace the whole floor plan (delete-all + insert each room as a JSON row).
/// Best-effort.
pub fn save(db: &Db, rooms: &[MapRoom]) {
    let pool = db.pool();
    let rooms = rooms.to_vec();
    let res: Result<(), sqlx::Error> = db.block(async move {
        let mut tx = pool.begin().await?;
        sqlx::query("DELETE FROM map_rooms").execute(&mut *tx).await?;
        for r in &rooms {
            let data = serde_json::to_value(r).unwrap_or_else(|_| serde_json::json!({}));
            sqlx::query("INSERT INTO map_rooms (id, data) VALUES ($1, $2)")
                .bind(r.id as i64)
                .bind(&data)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(())
    });
    if let Err(e) = res {
        log::error!("map::repo::save transaction failed: {e}");
    }
}
