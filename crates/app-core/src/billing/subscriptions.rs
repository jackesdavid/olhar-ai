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

/// SQL that folds a lapsed trial into the reported status.
///
/// Nothing pushes an event when a trial ends (it exists only in our DB — the payment
/// provider never knew about it), so a periodic sweep alone would leave a window where an
/// expired trial still had access. We therefore resolve it **at read time**, and the sweep
/// only keeps the stored `status` honest for reporting.
///
/// Deliberately evaluated in Postgres, not Rust: `ended_at` is RFC3339 text, and
/// `::timestamptz` is the same cast the sweep (migration 0045) uses — one source of truth.
/// Parsing the text in Rust meant an unexpected format silently *granted* access.
const EFFECTIVE_STATUS: &str = "CASE WHEN status = 'trialing' AND ended_at IS NOT NULL \
     AND ended_at::timestamptz < now() THEN 'expired' ELSE status END AS status";

/// The tenant's current subscription (newest row), if any. A lapsed trial is reported as
/// `expired` even before the sweep has rewritten the row (see [`EFFECTIVE_STATUS`]).
pub fn current(db: &Db, tenant_id: Uuid) -> Option<Subscription> {
    let pool = db.pool();
    let sql = format!(
        "SELECT id, tenant_id, plan_id, {EFFECTIVE_STATUS}, started_at, ended_at
         FROM tenant_subscriptions WHERE tenant_id = $1
         ORDER BY started_at DESC LIMIT 1"
    );
    db.block(async move {
        sqlx::query_as::<_, Subscription>(&sql)
            .bind(tenant_id)
            .fetch_optional(&pool)
            .await
            .ok()
            .flatten()
    })
}

/// `[start, end)` instants of a `YYYY-MM` period, as UTC RFC3339.
fn period_bounds(period: &str) -> Option<(String, String)> {
    use chrono::Datelike;
    let start = chrono::NaiveDate::parse_from_str(&format!("{period}-01"), "%Y-%m-%d").ok()?;
    let next = if start.month() == 12 {
        chrono::NaiveDate::from_ymd_opt(start.year() + 1, 1, 1)?
    } else {
        chrono::NaiveDate::from_ymd_opt(start.year(), start.month() + 1, 1)?
    };
    Some((format!("{start}T00:00:00Z"), format!("{next}T00:00:00Z")))
}

/// The subscription that was in force during `period` (`YYYY-MM`), i.e. whose span overlaps
/// it. `None` when the tenant wasn't a customer then — billing such a period would invoice
/// someone for a month they hadn't subscribed, and the invoice is immutable once `final`.
///
/// Use this (not [`current`]) to bill a past period: `current` would apply today's plan to
/// a month the tenant may never have had.
pub fn active_during(db: &Db, tenant_id: Uuid, period: &str) -> Option<Subscription> {
    let (start, end) = period_bounds(period)?;
    let pool = db.pool();
    let sql = format!(
        "SELECT id, tenant_id, plan_id, {EFFECTIVE_STATUS}, started_at, ended_at
         FROM tenant_subscriptions
         WHERE tenant_id = $1
           AND started_at::timestamptz < $3::timestamptz
           AND (ended_at IS NULL OR ended_at::timestamptz >= $2::timestamptz)
         ORDER BY started_at DESC LIMIT 1"
    );
    db.block(async move {
        sqlx::query_as::<_, Subscription>(&sql)
            .bind(tenant_id)
            .bind(&start)
            .bind(&end)
            .fetch_optional(&pool)
            .await
            .ok()
            .flatten()
    })
}

/// Statuses that soft-lock a tenant (read-only + upgrade prompt).
pub const LOCKED_STATUSES: [&str; 3] = ["expired", "past_due", "canceled"];

/// The tenant's effective subscription status, with a lapsed trial already resolved.
/// `None` when the tenant has no subscription at all (pre-billing tenants stay unlocked).
pub fn effective_status(db: &Db, tenant_id: Uuid) -> Option<String> {
    current(db, tenant_id).map(|s| s.status)
}

/// Whether the tenant is soft-locked (writes denied, reads still allowed).
pub fn is_locked(db: &Db, tenant_id: Uuid) -> bool {
    effective_status(db, tenant_id)
        .map(|s| LOCKED_STATUSES.contains(&s.as_str()))
        .unwrap_or(false)
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
    let pool = db.pool();
    db.block(async move {
        // `ended_at::timestamptz` (not a text `<`): RFC3339 strings only sort correctly
        // when their UTC offsets match. Same cast the pg_cron sweep uses.
        sqlx::query(
            "UPDATE tenant_subscriptions SET status = 'expired'
             WHERE status = 'trialing' AND ended_at IS NOT NULL
               AND ended_at::timestamptz < now()",
        )
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

/// One row of the master billing overview: a tenant, its billing currency, and its current
/// plan (if any). `base_fee` is the tier's price **in the tenant's currency**.
#[derive(Debug, Clone, FromRow, serde::Serialize)]
pub struct TenantBilling {
    pub tenant_id: Uuid,
    pub tenant_name: String,
    pub currency: String,
    pub plan_id: Option<String>,
    pub plan_name: Option<String>,
    pub base_fee: Option<f64>,
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
            "SELECT t.id AS tenant_id, t.name AS tenant_name, t.currency,
                    s.plan_id, p.name AS plan_name, pp.amount AS base_fee, s.status, s.ended_at
             FROM tenants t
             LEFT JOIN LATERAL (
                 SELECT plan_id, status, ended_at FROM tenant_subscriptions ts
                 WHERE ts.tenant_id = t.id ORDER BY started_at DESC LIMIT 1
             ) s ON true
             LEFT JOIN billing_plans p ON p.id = s.plan_id
             LEFT JOIN plan_prices pp ON pp.plan_id = s.plan_id AND pp.currency = t.currency
             ORDER BY t.name",
        )
        .fetch_all(&pool)
        .await
        .unwrap_or_default()
    })
}
