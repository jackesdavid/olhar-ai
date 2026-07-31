//! Tenancy basics shared across the workspace. Every domain row is scoped to a
//! `tenant_id`; the repos take it explicitly and never derive it from client input.
//!
//! Single-tenant callers (the desktop shell, sentinel-flow today) pass
//! [`DEFAULT_TENANT_ID`] — the same well-known tenant the migration backfills existing
//! rows to. The cloud APIs pass the real tenant resolved from the JWT.
use uuid::Uuid;

use crate::db::Db;

/// The "Default" tenant (`11111111-1111-1111-1111-111111111111`) seeded by migration
/// 0011 and used by single-tenant callers + the data backfill.
pub const DEFAULT_TENANT_ID: Uuid = Uuid::from_u128(0x1111_1111_1111_1111_1111_1111_1111_1111);

/// The tenant's most-recently-seen edge box host (MagicDNS), or `None`. Mirrors the
/// gateway/control live-upstream lookup over `tenant_boxes.magicdns_host`. Used by
pub fn box_host_for_tenant(db: &Db, tenant_id: Uuid) -> Option<String> {
    let pool = db.pool();
    db.block(async move {
        sqlx::query_as::<_, (Option<String>,)>(
            "SELECT magicdns_host FROM tenant_boxes \
              WHERE tenant_id = $1 AND magicdns_host IS NOT NULL \
           ORDER BY last_seen DESC NULLS LAST LIMIT 1",
        )
        .bind(tenant_id)
        .fetch_optional(&pool)
        .await
        .ok()
        .flatten()
        .and_then(|(h,)| h)
    })
}
