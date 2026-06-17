//! Postgres adapter for the map slice — owns the floor-plan SQL (moved out of the
//! shared `db.rs`). Tenant-scoped: rooms keyed `(tenant_id, id)`.
use uuid::Uuid;

use crate::db::Db;

use super::{normalize_rooms, MapDoc, MapMarker, MapRoom, MapWall};

/// The tenant's floor-plan rooms, ordered by id, with camera offsets normalized.
/// Malformed rows are skipped. Empty on a query error.
pub fn load(db: &Db, tenant_id: Uuid) -> Vec<MapRoom> {
    let pool = db.pool();
    db.block(async move {
        let rows = sqlx::query_as::<_, (serde_json::Value,)>(
            "SELECT data FROM map_rooms WHERE tenant_id = $1 ORDER BY id",
        )
        .bind(tenant_id)
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

/// Replace the tenant's floor plan (delete-all-for-tenant + insert each room).
/// Best-effort.
pub fn save(db: &Db, tenant_id: Uuid, rooms: &[MapRoom]) {
    let pool = db.pool();
    let rooms = rooms.to_vec();
    let res: Result<(), sqlx::Error> = db.block(async move {
        let mut tx = pool.begin().await?;
        sqlx::query("DELETE FROM map_rooms WHERE tenant_id = $1").bind(tenant_id).execute(&mut *tx).await?;
        for r in &rooms {
            let data = serde_json::to_value(r).unwrap_or_else(|_| serde_json::json!({}));
            sqlx::query("INSERT INTO map_rooms (tenant_id, id, data) VALUES ($1, $2, $3)")
                .bind(tenant_id)
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

/// The tenant's free-form walls (one jsonb row each). Malformed rows skipped.
pub fn load_walls(db: &Db, tenant_id: Uuid) -> Vec<MapWall> {
    let pool = db.pool();
    db.block(async move {
        let rows = sqlx::query_as::<_, (serde_json::Value,)>(
            "SELECT data FROM map_walls WHERE tenant_id = $1 ORDER BY id",
        )
        .bind(tenant_id)
        .fetch_all(&pool)
        .await;
        match rows {
            Ok(rows) => rows.into_iter().filter_map(|(v,)| serde_json::from_value::<MapWall>(v).ok()).collect(),
            Err(e) => {
                log::error!("map::repo::load_walls query failed: {e}");
                Vec::new()
            }
        }
    })
}

/// The tenant's markers (furniture / landmark glyphs). Malformed rows skipped.
pub fn load_markers(db: &Db, tenant_id: Uuid) -> Vec<MapMarker> {
    let pool = db.pool();
    db.block(async move {
        let rows = sqlx::query_as::<_, (serde_json::Value,)>(
            "SELECT data FROM map_markers WHERE tenant_id = $1 ORDER BY id",
        )
        .bind(tenant_id)
        .fetch_all(&pool)
        .await;
        match rows {
            Ok(rows) => rows.into_iter().filter_map(|(v,)| serde_json::from_value::<MapMarker>(v).ok()).collect(),
            Err(e) => {
                log::error!("map::repo::load_markers query failed: {e}");
                Vec::new()
            }
        }
    })
}

/// Load the whole floor-plan document (rooms + walls + markers) in one place.
pub fn load_doc(db: &Db, tenant_id: Uuid) -> MapDoc {
    MapDoc {
        rooms: load(db, tenant_id),
        walls: load_walls(db, tenant_id),
        markers: load_markers(db, tenant_id),
    }
}

/// Replace the tenant's whole floor plan (rooms + walls + markers) atomically.
pub fn save_doc(db: &Db, tenant_id: Uuid, doc: &MapDoc) {
    let pool = db.pool();
    let doc = doc.clone();
    let res: Result<(), sqlx::Error> = db.block(async move {
        let mut tx = pool.begin().await?;

        sqlx::query("DELETE FROM map_rooms WHERE tenant_id = $1").bind(tenant_id).execute(&mut *tx).await?;
        for r in &doc.rooms {
            let data = serde_json::to_value(r).unwrap_or_else(|_| serde_json::json!({}));
            sqlx::query("INSERT INTO map_rooms (tenant_id, id, data) VALUES ($1, $2, $3)")
                .bind(tenant_id)
                .bind(r.id as i64)
                .bind(&data)
                .execute(&mut *tx)
                .await?;
        }

        sqlx::query("DELETE FROM map_walls WHERE tenant_id = $1").bind(tenant_id).execute(&mut *tx).await?;
        for w in &doc.walls {
            let data = serde_json::to_value(w).unwrap_or_else(|_| serde_json::json!({}));
            sqlx::query("INSERT INTO map_walls (tenant_id, id, data) VALUES ($1, $2, $3)")
                .bind(tenant_id)
                .bind(w.id as i64)
                .bind(&data)
                .execute(&mut *tx)
                .await?;
        }

        sqlx::query("DELETE FROM map_markers WHERE tenant_id = $1").bind(tenant_id).execute(&mut *tx).await?;
        for m in &doc.markers {
            let data = serde_json::to_value(m).unwrap_or_else(|_| serde_json::json!({}));
            sqlx::query("INSERT INTO map_markers (tenant_id, id, data) VALUES ($1, $2, $3)")
                .bind(tenant_id)
                .bind(m.id as i64)
                .bind(&data)
                .execute(&mut *tx)
                .await?;
        }

        tx.commit().await?;
        Ok(())
    });
    if let Err(e) = res {
        log::error!("map::repo::save_doc transaction failed: {e}");
    }
}
