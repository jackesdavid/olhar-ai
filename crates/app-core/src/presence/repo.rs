//! Postgres adapter for the presence slice. Tenant-scoped, per-tenant monotonic id
//! (assigned in the INSERT). Mirrors `alerts::repo`.
use uuid::Uuid;

use crate::db::Db;

use super::{Category, Presence};

/// Insert one presence row, assigning the next per-tenant id. Returns the new id
/// (or `None` on error). Best-effort.
#[allow(clippy::too_many_arguments)]
pub fn insert(
    db: &Db,
    tenant_id: Uuid,
    category: Category,
    label: Option<&str>,
    room_id: Option<u32>,
    room_name: Option<&str>,
    camera: Option<&str>,
    source: &str,
    created_at: &str,
) -> Option<u64> {
    let pool = db.pool();
    let cat = category.slug().to_string();
    let label = label.map(str::to_string);
    let room_name = room_name.map(str::to_string);
    let camera = camera.map(str::to_string);
    let source = source.to_string();
    let created_at = created_at.to_string();
    let room_id = room_id.map(|r| r as i64);
    db.block(async move {
        let row: Result<(i64,), sqlx::Error> = sqlx::query_as(
            "INSERT INTO presence (tenant_id, id, category, label, room_id, room_name, camera, source, created_at) \
             VALUES ($1, (SELECT COALESCE(MAX(id), 0) + 1 FROM presence WHERE tenant_id = $1), $2, $3, $4, $5, $6, $7, $8) \
             RETURNING id",
        )
        .bind(tenant_id)
        .bind(&cat)
        .bind(&label)
        .bind(room_id)
        .bind(&room_name)
        .bind(&camera)
        .bind(&source)
        .bind(&created_at)
        .fetch_one(&pool)
        .await;
        match row {
            Ok((id,)) => Some(id as u64),
            Err(e) => {
                log::error!("presence::repo::insert failed: {e}");
                None
            }
        }
    })
}

/// The tenant's most-recent presence events, oldest-first (capped at `limit`).
pub fn load_recent(db: &Db, tenant_id: Uuid, limit: i64) -> Vec<Presence> {
    let pool = db.pool();
    db.block(async move {
        type Row = (i64, String, Option<String>, Option<i64>, Option<String>, Option<String>, String, String);
        let rows = sqlx::query_as::<_, Row>(
            "SELECT id, category, label, room_id, room_name, camera, source, created_at \
             FROM presence WHERE tenant_id = $1 ORDER BY id DESC LIMIT $2",
        )
        .bind(tenant_id)
        .bind(limit.max(1))
        .fetch_all(&pool)
        .await;
        match rows {
            Ok(mut rows) => {
                rows.reverse(); // chronological
                rows.into_iter()
                    .map(|(id, category, label, room_id, room_name, camera, source, created_at)| Presence {
                        id: id as u64,
                        category: Category::parse(&category),
                        label,
                        room_id: room_id.map(|r| r as u32),
                        room_name,
                        camera,
                        source,
                        created_at,
                    })
                    .collect()
            }
            Err(e) => {
                log::error!("presence::repo::load_recent query failed: {e}");
                Vec::new()
            }
        }
    })
}
