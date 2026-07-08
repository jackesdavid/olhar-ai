//! The subscription-plan catalog (`billing_plans`). A plan is a fixed BRL base fee, a
//! camera cap, an optional trial window, and included-quota / overage-rate columns
//! (all `0` for the v1 flat tiers → invoice = base fee).

use sqlx::FromRow;

use crate::db::Db;

/// One subscription plan. `id` is a slug (`beta` / `home` / `business`).
#[derive(Debug, Clone, FromRow, serde::Serialize)]
pub struct Plan {
    pub id: String,
    pub name: String,
    pub base_fee_brl: f64,
    pub camera_cap: i32,
    /// `Some(n)` for a trial plan (n-day window), `None` for a paid plan.
    pub trial_days: Option<i32>,
    pub included_gpu_sec: f64,
    pub included_storage_gb: f64,
    pub included_notifications: i32,
    pub overage_gpu_per_sec: f64,
    pub overage_storage_per_gb: f64,
    pub overage_notification_each: f64,
    pub active: bool,
}

/// All active plans, cheapest first.
pub fn list_active(db: &Db) -> Vec<Plan> {
    let pool = db.pool();
    db.block(async move {
        sqlx::query_as::<_, Plan>("SELECT * FROM billing_plans WHERE active ORDER BY base_fee_brl")
            .fetch_all(&pool)
            .await
            .unwrap_or_default()
    })
}

/// One plan by its slug id.
pub fn get(db: &Db, id: &str) -> Option<Plan> {
    let pool = db.pool();
    let id = id.to_string();
    db.block(async move {
        sqlx::query_as::<_, Plan>("SELECT * FROM billing_plans WHERE id = $1")
            .bind(&id)
            .fetch_optional(&pool)
            .await
            .ok()
            .flatten()
    })
}
