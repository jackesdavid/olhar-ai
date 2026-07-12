//! Storage visibility (card #64, Phase 5).
//!
//! Two questions this answers, for the master across every tenant and for a tenant about
//! itself: **how much footage is durable right now** (the stock), and **what will it cost this
//! period** (GB-month against the quota). The stock is what the customer sees "used"; GB-month —
//! the average occupancy the nightly sampler records — is what the invoice actually charges, so
//! both are surfaced rather than conflated.
//!
//! This is the view that catches a tenant driving the edge to record continuously *before* the
//! invoice does: sort the master list by GB-month and the outlier is at the top.

use serde::Serialize;
use uuid::Uuid;

use crate::db::Db;

#[derive(Serialize, sqlx::FromRow)]
pub struct TenantStorage {
    pub tenant_id: Uuid,
    pub tenant_name: String,
    pub currency: String,
    /// Current plan slug, `None` for a tenant with no subscription.
    pub plan_id: Option<String>,
    pub plan_name: Option<String>,
    /// Cloud retention the plan sells, in days. `0` = no cloud recording.
    pub retention_days: i32,
    /// Quota included in the plan, GB. `0` when the tenant has no plan.
    pub included_storage_gb: f64,
    /// Overage rate in the tenant's currency, per GB-month over quota.
    pub overage_per_gb: f64,
    /// Durable footage **right now**: bytes of uploaded clips in the bucket.
    pub stored_bytes: i64,
    /// Uploaded clip count — context for the byte figure.
    pub clip_count: i64,
    /// Average GB stored across this period's sampled days — the billed quantity (GB-month).
    pub gb_month: f64,
}

impl TenantStorage {
    /// Billable overage this period: GB-month over quota × rate. Never negative.
    pub fn projected_overage(&self) -> f64 {
        (self.gb_month - self.included_storage_gb).max(0.0) * self.overage_per_gb
    }
}

/// `YYYY-MM` for the current month in UTC — the period the sampler is filling.
fn current_period(db: &Db) -> String {
    let pool = db.pool();
    db.block(async move {
        sqlx::query_scalar::<_, String>("SELECT to_char(now(), 'YYYY-MM')")
            .fetch_one(&pool)
            .await
            .unwrap_or_default()
    })
}

/// The SELECT both views share. `$1` is the period prefix (`'2026-07'`) for the GB-month average;
/// `$2` narrows to a single tenant (pass `None` for all).
///
/// Stored bytes and clip count come from `recordings` (the live stock); GB-month comes from
/// `tenant_storage_daily` (what the nightly pg_cron sampler recorded), so the invoice and this
/// view agree on the billed quantity. `LEFT JOIN` on subscription/plan/price keeps tenants with
/// no plan — a Beta or unsubscribed tenant can still be storing footage we pay for.
fn query(db: &Db, tenant_id: Option<Uuid>) -> Vec<TenantStorage> {
    let period = current_period(db);
    let pool = db.pool();
    db.block(async move {
        sqlx::query_as::<_, TenantStorage>(
            "SELECT t.id AS tenant_id, t.name AS tenant_name, t.currency,
                    s.plan_id, p.name AS plan_name,
                    COALESCE(p.retention_days, 0)      AS retention_days,
                    COALESCE(p.included_storage_gb, 0) AS included_storage_gb,
                    COALESCE(pp.overage_storage_per_gb, 0) AS overage_per_gb,
                    -- SUM(bigint) and AVG(...) come back as `numeric`, which this sqlx build
                    -- (no numeric decoder) can't read — an uncast column errors the whole
                    -- fetch and empties the view. Cast to the struct's i64 / f64 at the source.
                    COALESCE(r.stored_bytes, 0)::bigint           AS stored_bytes,
                    COALESCE(r.clip_count, 0)                     AS clip_count,
                    COALESCE(d.gb_month, 0)::double precision     AS gb_month
             FROM tenants t
             LEFT JOIN LATERAL (
                 SELECT plan_id, status FROM tenant_subscriptions ts
                 WHERE ts.tenant_id = t.id ORDER BY started_at DESC LIMIT 1
             ) s ON true
             LEFT JOIN billing_plans p ON p.id = s.plan_id
             LEFT JOIN plan_prices pp ON pp.plan_id = s.plan_id AND pp.currency = t.currency
             LEFT JOIN LATERAL (
                 SELECT SUM(size_bytes) AS stored_bytes, COUNT(*) AS clip_count
                 FROM recordings
                 WHERE tenant_id = t.id AND cloud_key IS NOT NULL AND upload_state = 'uploaded'
             ) r ON true
             LEFT JOIN LATERAL (
                 SELECT AVG(bytes) / 1e9 AS gb_month
                 FROM tenant_storage_daily
                 WHERE tenant_id = t.id AND day::text LIKE $1 || '%'
             ) d ON true
             WHERE ($2::uuid IS NULL OR t.id = $2)
             ORDER BY d.gb_month DESC NULLS LAST, t.name",
        )
        .bind(format!("{period}"))
        .bind(tenant_id)
        .fetch_all(&pool)
        .await
        .unwrap_or_default()
    })
}

/// Every tenant's storage, heaviest first — the master's cross-tenant view.
pub fn overview(db: &Db) -> Vec<TenantStorage> {
    query(db, None)
}

/// One tenant's own storage, for the tenant-facing usage strip.
pub fn for_tenant(db: &Db, tenant_id: Uuid) -> Option<TenantStorage> {
    query(db, Some(tenant_id)).into_iter().next()
}
