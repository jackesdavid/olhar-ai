//! The subscription-plan catalog. A [`Plan`] is the **tier** — camera cap, trial window,
//! included quotas — and carries no money. Prices live per (plan, currency) in
//! `plan_prices`, so the same tier sells in BRL (Brazil) and USD (American market).
//! Provider-side price objects (a Stripe `price_...`) live in `plan_provider_price`.

use sqlx::FromRow;

use crate::db::Db;

/// One subscription tier. `id` is a slug (`beta` / `home` / `business`).
#[derive(Debug, Clone, FromRow, serde::Serialize, serde::Deserialize)]
pub struct Plan {
    pub id: String,
    pub name: String,
    pub camera_cap: i32,
    /// `Some(n)` for a trial plan (n-day window), `None` for a paid plan.
    pub trial_days: Option<i32>,
    /// Days of cloud recording retention this tier sells. `0` = no cloud recording.
    /// Retention is the product; the quota below is only a guardrail.
    pub retention_days: i32,
    pub included_gpu_sec: f64,
    pub included_storage_gb: f64,
    pub included_notifications: i32,
    pub active: bool,
}

/// A tier priced in one currency — what the UI and invoices actually consume.
///
/// Quotas (`included_*`) come from the tier: a GB is a GB in any currency. The rates come from
/// the price row, because a rate is money.
#[derive(Debug, Clone, FromRow, serde::Serialize)]
pub struct PricedPlan {
    pub id: String,
    pub name: String,
    pub camera_cap: i32,
    pub trial_days: Option<i32>,
    pub retention_days: i32,
    pub included_gpu_sec: f64,
    pub included_storage_gb: f64,
    pub included_notifications: i32,
    pub active: bool,
    pub currency: String,
    pub amount: f64,
    pub overage_gpu_per_sec: f64,
    pub overage_storage_per_gb: f64,
    pub overage_notification_each: f64,
}

/// One (plan, currency) price.
#[derive(Debug, Clone, FromRow, serde::Serialize, serde::Deserialize)]
pub struct PlanPrice {
    pub plan_id: String,
    pub currency: String,
    pub amount: f64,
    pub overage_storage_per_gb: f64,
}

/// Active tiers priced in `currency`, cheapest first.
pub fn list_active_priced(db: &Db, currency: &str) -> Vec<PricedPlan> {
    let pool = db.pool();
    let currency = currency.to_string();
    db.block(async move {
        sqlx::query_as::<_, PricedPlan>(
            "SELECT p.*, pp.currency, pp.amount, pp.overage_gpu_per_sec,
                    pp.overage_storage_per_gb, pp.overage_notification_each
             FROM billing_plans p
             JOIN plan_prices pp ON pp.plan_id = p.id AND pp.currency = $1
             WHERE p.active ORDER BY pp.amount",
        )
        .bind(&currency)
        .fetch_all(&pool)
        .await
        .unwrap_or_default()
    })
}

/// One tier priced in `currency` — the shape the invoice bills from.
pub fn priced(db: &Db, plan_id: &str, currency: &str) -> Option<PricedPlan> {
    let pool = db.pool();
    let plan_id = plan_id.to_string();
    let currency = currency.to_string();
    db.block(async move {
        sqlx::query_as::<_, PricedPlan>(
            "SELECT p.*, pp.currency, pp.amount, pp.overage_gpu_per_sec,
                    pp.overage_storage_per_gb, pp.overage_notification_each
             FROM billing_plans p
             JOIN plan_prices pp ON pp.plan_id = p.id AND pp.currency = $2
             WHERE p.id = $1",
        )
        .bind(&plan_id)
        .bind(&currency)
        .fetch_optional(&pool)
        .await
        .ok()
        .flatten()
    })
}

/// One tier by slug (no price).
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

/// A tier plus every currency it is priced in (master catalog view).
#[derive(Debug, Clone, serde::Serialize)]
pub struct PlanWithPrices {
    #[serde(flatten)]
    pub plan: Plan,
    pub prices: Vec<PlanPrice>,
}

/// Every tier with its per-currency prices. Handful of plans → the per-plan price
/// lookup is fine.
pub fn list_all_with_prices(db: &Db) -> Vec<PlanWithPrices> {
    list_all(db)
        .into_iter()
        .map(|plan| {
            let prices = prices(db, &plan.id);
            PlanWithPrices { plan, prices }
        })
        .collect()
}

/// Every tier, active or not (master catalog view).
pub fn list_all(db: &Db) -> Vec<Plan> {
    let pool = db.pool();
    db.block(async move {
        sqlx::query_as::<_, Plan>("SELECT * FROM billing_plans ORDER BY camera_cap")
            .fetch_all(&pool)
            .await
            .unwrap_or_default()
    })
}

/// The price of `plan_id` in `currency`, if set.
pub fn price(db: &Db, plan_id: &str, currency: &str) -> Option<f64> {
    let pool = db.pool();
    let plan_id = plan_id.to_string();
    let currency = currency.to_string();
    db.block(async move {
        sqlx::query_scalar::<_, f64>(
            "SELECT amount FROM plan_prices WHERE plan_id = $1 AND currency = $2",
        )
        .bind(&plan_id)
        .bind(&currency)
        .fetch_optional(&pool)
        .await
        .ok()
        .flatten()
    })
}

/// Every currency a plan is priced in.
pub fn prices(db: &Db, plan_id: &str) -> Vec<PlanPrice> {
    let pool = db.pool();
    let plan_id = plan_id.to_string();
    db.block(async move {
        sqlx::query_as::<_, PlanPrice>(
            "SELECT plan_id, currency, amount, overage_storage_per_gb
             FROM plan_prices WHERE plan_id = $1 ORDER BY currency",
        )
        .bind(&plan_id)
        .fetch_all(&pool)
        .await
        .unwrap_or_default()
    })
}

/// Create or update a tier (master plan administration).
pub fn upsert(db: &Db, plan: Plan) -> Result<(), String> {
    let pool = db.pool();
    let res: Result<_, sqlx::Error> = db.block(async move {
        sqlx::query(
            "INSERT INTO billing_plans
                (id, name, camera_cap, trial_days, retention_days, included_gpu_sec,
                 included_storage_gb, included_notifications, active)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)
             ON CONFLICT (id) DO UPDATE SET
                name = $2, camera_cap = $3, trial_days = $4, retention_days = $5,
                included_gpu_sec = $6, included_storage_gb = $7,
                included_notifications = $8, active = $9",
        )
        .bind(&plan.id)
        .bind(&plan.name)
        .bind(plan.camera_cap)
        .bind(plan.trial_days)
        .bind(plan.retention_days)
        .bind(plan.included_gpu_sec)
        .bind(plan.included_storage_gb)
        .bind(plan.included_notifications)
        .bind(plan.active)
        .execute(&pool)
        .await
    });
    res.map(|_| ()).map_err(|e| format!("plans::upsert: {e}"))
}

/// Set one (plan, currency) price and its storage overage rate — both money, one row.
pub fn set_price(db: &Db, plan_id: &str, currency: &str, amount: f64, overage_storage_per_gb: f64) -> Result<(), String> {
    let plan_id = plan_id.to_string();
    let currency = currency.to_string();
    let pool = db.pool();
    let res: Result<_, sqlx::Error> = db.block(async move {
        sqlx::query(
            "INSERT INTO plan_prices (plan_id, currency, amount, overage_storage_per_gb)
             VALUES ($1, $2, $3, $4)
             ON CONFLICT (plan_id, currency)
             DO UPDATE SET amount = $3, overage_storage_per_gb = $4",
        )
        .bind(&plan_id)
        .bind(&currency)
        .bind(amount)
        .bind(overage_storage_per_gb)
        .execute(&pool)
        .await
    });
    res.map(|_| ()).map_err(|e| format!("plans::set_price: {e}"))
}

/// The provider's price-object ref for a plan (e.g. a Stripe `price_...`).
pub fn provider_price_ref(db: &Db, plan_id: &str, provider: &str) -> Option<String> {
    let plan_id = plan_id.to_string();
    let provider = provider.to_string();
    let pool = db.pool();
    db.block(async move {
        sqlx::query_scalar::<_, String>(
            "SELECT price_ref FROM plan_provider_price WHERE plan_id = $1 AND provider = $2",
        )
        .bind(&plan_id)
        .bind(&provider)
        .fetch_optional(&pool)
        .await
        .ok()
        .flatten()
    })
}

/// Record the provider's price-object ref for a plan.
pub fn set_provider_price_ref(db: &Db, plan_id: &str, provider: &str, price_ref: &str) -> Result<(), String> {
    let plan_id = plan_id.to_string();
    let provider = provider.to_string();
    let price_ref = price_ref.to_string();
    let pool = db.pool();
    let res: Result<_, sqlx::Error> = db.block(async move {
        sqlx::query(
            "INSERT INTO plan_provider_price (plan_id, provider, price_ref) VALUES ($1, $2, $3)
             ON CONFLICT (plan_id, provider) DO UPDATE SET price_ref = $3",
        )
        .bind(&plan_id)
        .bind(&provider)
        .bind(&price_ref)
        .execute(&pool)
        .await
    });
    res.map(|_| ()).map_err(|e| format!("plans::set_provider_price_ref: {e}"))
}

/// The distinct retention windows any plan sells — one bucket lifecycle rule per tier.
pub fn retention_tiers(db: &Db) -> Vec<i32> {
    let pool = db.pool();
    db.block(async move {
        sqlx::query_scalar::<_, i32>(
            "SELECT DISTINCT retention_days FROM billing_plans WHERE retention_days > 0 ORDER BY 1",
        )
        .fetch_all(&pool)
        .await
        .unwrap_or_default()
    })
}
