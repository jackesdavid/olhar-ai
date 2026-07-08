//! The subscription-plan catalog (`billing_plans`). A plan is a fixed BRL base fee, a
//! camera cap, an optional trial window, and included-quota / overage-rate columns
//! (all `0` for the v1 flat tiers → invoice = base fee).

use sqlx::FromRow;

use crate::db::Db;

/// One subscription plan. `id` is a slug (`beta` / `home` / `business`).
#[derive(Debug, Clone, FromRow, serde::Serialize, serde::Deserialize)]
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

/// Every plan, active or not, cheapest first (master catalog view).
pub fn list_all(db: &Db) -> Vec<Plan> {
    let pool = db.pool();
    db.block(async move {
        sqlx::query_as::<_, Plan>("SELECT * FROM billing_plans ORDER BY base_fee_brl")
            .fetch_all(&pool)
            .await
            .unwrap_or_default()
    })
}

/// Create or update a plan by id (master plan administration).
pub fn upsert(db: &Db, plan: Plan) -> Result<(), String> {
    let pool = db.pool();
    let res: Result<_, sqlx::Error> = db.block(async move {
        sqlx::query(
            "INSERT INTO billing_plans
                (id, name, base_fee_brl, camera_cap, trial_days, included_gpu_sec,
                 included_storage_gb, included_notifications, overage_gpu_per_sec,
                 overage_storage_per_gb, overage_notification_each, active)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)
             ON CONFLICT (id) DO UPDATE SET
                name = $2, base_fee_brl = $3, camera_cap = $4, trial_days = $5,
                included_gpu_sec = $6, included_storage_gb = $7, included_notifications = $8,
                overage_gpu_per_sec = $9, overage_storage_per_gb = $10,
                overage_notification_each = $11, active = $12",
        )
        .bind(&plan.id)
        .bind(&plan.name)
        .bind(plan.base_fee_brl)
        .bind(plan.camera_cap)
        .bind(plan.trial_days)
        .bind(plan.included_gpu_sec)
        .bind(plan.included_storage_gb)
        .bind(plan.included_notifications)
        .bind(plan.overage_gpu_per_sec)
        .bind(plan.overage_storage_per_gb)
        .bind(plan.overage_notification_each)
        .bind(plan.active)
        .execute(&pool)
        .await
    });
    res.map(|_| ()).map_err(|e| format!("plans::upsert: {e}"))
}
