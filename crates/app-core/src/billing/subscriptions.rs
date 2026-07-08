//! Per-tenant subscriptions + the Beta trial lifecycle. Current plan = a tenant's newest
//! `tenant_subscriptions` row. A trial is `status='trialing'` with `ended_at =
//! started_at + trial_days`; the app soft-locks `expired`/`past_due`/`canceled` tenants.

use sqlx::FromRow;
use uuid::Uuid;

use crate::alerts::now_rfc3339;
use crate::db::Db;

/// Slug of the free trial plan (seeded by migration 0040).
pub const BETA_PLAN_ID: &str = "beta";

/// One (tenant, plan) span.
#[derive(Debug, Clone, FromRow, serde::Serialize)]
pub struct Subscription {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub plan_id: String,
    pub status: String,
    pub started_at: String,
    pub ended_at: Option<String>,
}

/// The tenant's current subscription (newest row), if any.
pub fn current(db: &Db, tenant_id: Uuid) -> Option<Subscription> {
    let pool = db.pool();
    db.block(async move {
        sqlx::query_as::<_, Subscription>(
            "SELECT id, tenant_id, plan_id, status, started_at, ended_at
             FROM tenant_subscriptions WHERE tenant_id = $1
             ORDER BY started_at DESC LIMIT 1",
        )
        .bind(tenant_id)
        .fetch_optional(&pool)
        .await
        .ok()
        .flatten()
    })
}

/// Start (or, for a master-granted re-trial, restart) the Beta trial: a `trialing` row
/// ending `trial_days` from now. Used by self-service signup and the master console.
pub fn start_trial(db: &Db, tenant_id: Uuid) -> Result<Uuid, String> {
    let plan = super::plans::get(db, BETA_PLAN_ID).ok_or("Beta plan not found")?;
    let days = plan.trial_days.unwrap_or(30).max(0) as i64;
    let id = Uuid::new_v4();
    let start = chrono::Local::now();
    let started_at = start.to_rfc3339();
    let ended_at = (start + chrono::Duration::days(days)).to_rfc3339();
    let pool = db.pool();
    let res: Result<_, sqlx::Error> = db.block(async move {
        sqlx::query(
            "INSERT INTO tenant_subscriptions (id, tenant_id, plan_id, status, started_at, ended_at)
             VALUES ($1, $2, $3, 'trialing', $4, $5)",
        )
        .bind(id)
        .bind(tenant_id)
        .bind(BETA_PLAN_ID)
        .bind(&started_at)
        .bind(&ended_at)
        .execute(&pool)
        .await
    });
    res.map(|_| id).map_err(|e| format!("start_trial: {e}"))
}

/// Move a tenant onto a paid plan: close the open subscription and open an `active` one.
pub fn set_plan(db: &Db, tenant_id: Uuid, plan_id: &str) -> Result<Uuid, String> {
    super::plans::get(db, plan_id).ok_or("plan not found")?;
    let id = Uuid::new_v4();
    let now = now_rfc3339();
    let plan_id = plan_id.to_string();
    let pool = db.pool();
    let res: Result<_, sqlx::Error> = db.block(async move {
        let mut tx = pool.begin().await?;
        sqlx::query(
            "UPDATE tenant_subscriptions SET ended_at = $2
             WHERE tenant_id = $1 AND ended_at IS NULL",
        )
        .bind(tenant_id)
        .bind(&now)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO tenant_subscriptions (id, tenant_id, plan_id, status, started_at, ended_at)
             VALUES ($1, $2, $3, 'active', $4, NULL)",
        )
        .bind(id)
        .bind(tenant_id)
        .bind(&plan_id)
        .bind(&now)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok::<_, sqlx::Error>(())
    });
    res.map(|_| id).map_err(|e| format!("set_plan: {e}"))
}

/// Flip due trials `trialing → expired` (past `ended_at`). Returns how many expired.
/// Run periodically; the app soft-locks `expired` tenants (read-only + upgrade prompt).
pub fn expire_due_trials(db: &Db) -> u64 {
    let now = now_rfc3339();
    let pool = db.pool();
    db.block(async move {
        sqlx::query(
            "UPDATE tenant_subscriptions SET status = 'expired'
             WHERE status = 'trialing' AND ended_at IS NOT NULL AND ended_at < $1",
        )
        .bind(&now)
        .execute(&pool)
        .await
        .map(|r| r.rows_affected())
        .unwrap_or(0)
    })
}

/// The camera cap for a tenant's current plan. Falls back to the Beta cap (2) for a
/// tenant with no subscription yet, so signup can't exceed it before a plan is chosen.
pub fn camera_cap(db: &Db, tenant_id: Uuid) -> i32 {
    current(db, tenant_id)
        .and_then(|s| super::plans::get(db, &s.plan_id))
        .map(|p| p.camera_cap)
        .unwrap_or(2)
}

/// One row of the master billing overview: a tenant and its current plan (if any).
#[derive(Debug, Clone, FromRow, serde::Serialize)]
pub struct TenantBilling {
    pub tenant_id: Uuid,
    pub tenant_name: String,
    pub plan_id: Option<String>,
    pub plan_name: Option<String>,
    pub base_fee_brl: Option<f64>,
    pub status: Option<String>,
    /// Trial end (RFC3339) when the current subscription is a trial.
    pub ended_at: Option<String>,
}

/// Every tenant with its current plan + status (master cross-tenant view). Tenants with no
/// subscription appear with NULL plan fields.
pub fn overview(db: &Db) -> Vec<TenantBilling> {
    let pool = db.pool();
    db.block(async move {
        sqlx::query_as::<_, TenantBilling>(
            "SELECT t.id AS tenant_id, t.name AS tenant_name,
                    s.plan_id, p.name AS plan_name, p.base_fee_brl, s.status, s.ended_at
             FROM tenants t
             LEFT JOIN LATERAL (
                 SELECT plan_id, status, ended_at FROM tenant_subscriptions ts
                 WHERE ts.tenant_id = t.id ORDER BY started_at DESC LIMIT 1
             ) s ON true
             LEFT JOIN billing_plans p ON p.id = s.plan_id
             ORDER BY t.name",
        )
        .fetch_all(&pool)
        .await
        .unwrap_or_default()
    })
}
