//! Payments — the **provider seam**. The billing core (plans / invoices / usage) is
//! provider-agnostic; only charge creation + webhook handling live behind
//! [`PaymentGateway`]. [`asaas`] is the first (Brazil) implementation; a `stripe` impl can
//! be added for international selling without touching the core — pick via `PAYMENT_PROVIDER`.

pub mod asaas;

use uuid::Uuid;

use crate::alerts::now_rfc3339;
use crate::db::Db;

/// A normalized webhook event: which provider subscription changed, and to what status.
pub struct PaymentEvent {
    pub subscription_ref: String,
    /// Maps to `tenant_subscriptions.status`: "active" | "past_due" | "canceled".
    pub status: &'static str,
}

/// A payment provider — wraps one gateway's REST API + webhook format.
pub trait PaymentGateway: Send + Sync {
    fn provider(&self) -> &'static str;
    /// Ensure a provider-side customer exists; return its ref.
    fn ensure_customer(&self, name: &str, email: &str, cpf_cnpj: Option<&str>) -> Result<String, String>;
    /// Create a monthly subscription for `amount` (in the tenant's currency); return the ref.
    fn create_subscription(&self, customer_ref: &str, amount: f64, billing_type: &str, description: &str) -> Result<String, String>;
    /// The hosted checkout / Pix-boleto URL for the subscription's first charge, so the
    /// customer can actually pay. `None` if unavailable (default for providers without one).
    fn first_payment_url(&self, _subscription_ref: &str) -> Option<String> {
        None
    }
    /// Whether an inbound webhook carrying `token` is authentic.
    fn verify_webhook(&self, token: Option<&str>) -> bool;
    /// Parse a webhook body into a normalized event (`None` for events we ignore).
    fn parse_event(&self, body: &serde_json::Value) -> Option<PaymentEvent>;
}

/// The configured gateway (`PAYMENT_PROVIDER`, default `asaas`). `None` if unconfigured.
pub fn active_gateway() -> Option<Box<dyn PaymentGateway>> {
    let provider = std::env::var("PAYMENT_PROVIDER").unwrap_or_else(|_| "asaas".to_string());
    match provider.as_str() {
        "asaas" => asaas::AsaasGateway::from_env().map(|g| Box::new(g) as Box<dyn PaymentGateway>),
        // "stripe" => stripe::StripeGateway::from_env().map(...),  // international, future
        other => {
            log::warn!("payments: unknown PAYMENT_PROVIDER '{other}'");
            None
        }
    }
}

// ---- repo ----

/// Store (or refresh) the provider customer ref for a tenant.
pub fn upsert_customer(db: &Db, tenant_id: Uuid, provider: &str, customer_ref: &str) {
    let now = now_rfc3339();
    let provider = provider.to_string();
    let customer_ref = customer_ref.to_string();
    let pool = db.pool();
    let res: Result<_, sqlx::Error> = db.block(async move {
        sqlx::query(
            "INSERT INTO payment_customers (tenant_id, provider, customer_ref, created_at)
             VALUES ($1, $2, $3, $4)
             ON CONFLICT (tenant_id) DO UPDATE SET provider = $2, customer_ref = $3",
        )
        .bind(tenant_id)
        .bind(&provider)
        .bind(&customer_ref)
        .bind(&now)
        .execute(&pool)
        .await
    });
    if let Err(e) = res {
        log::error!("payments::upsert_customer failed: {e}");
    }
}

/// The tenant's provider customer ref, if one was stored.
pub fn customer_ref(db: &Db, tenant_id: Uuid, provider: &str) -> Option<String> {
    let provider = provider.to_string();
    let pool = db.pool();
    db.block(async move {
        sqlx::query_scalar::<_, String>(
            "SELECT customer_ref FROM payment_customers WHERE tenant_id = $1 AND provider = $2",
        )
        .bind(tenant_id)
        .bind(&provider)
        .fetch_optional(&pool)
        .await
        .ok()
        .flatten()
    })
}

/// Record a newly created provider subscription (status starts `active`).
pub fn record_subscription(db: &Db, tenant_id: Uuid, provider: &str, subscription_ref: &str, plan_id: &str, billing_type: &str) {
    let id = Uuid::new_v4();
    let now = now_rfc3339();
    let provider = provider.to_string();
    let subscription_ref = subscription_ref.to_string();
    let plan_id = plan_id.to_string();
    let billing_type = billing_type.to_string();
    let pool = db.pool();
    let res: Result<_, sqlx::Error> = db.block(async move {
        sqlx::query(
            "INSERT INTO payment_subscriptions
                (id, tenant_id, provider, subscription_ref, plan_id, billing_type, status, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, 'active', $7, $7)",
        )
        .bind(id)
        .bind(tenant_id)
        .bind(&provider)
        .bind(&subscription_ref)
        .bind(&plan_id)
        .bind(&billing_type)
        .bind(&now)
        .execute(&pool)
        .await
    });
    if let Err(e) = res {
        log::error!("payments::record_subscription failed: {e}");
    }
}

/// Apply a normalized webhook event: update the payment subscription **and** the tenant's
/// billing subscription status (which gates access). Returns the affected tenant, if found.
pub fn apply_event(db: &Db, provider: &str, ev: &PaymentEvent) -> Option<Uuid> {
    let now = now_rfc3339();
    let provider = provider.to_string();
    let sub_ref = ev.subscription_ref.clone();
    let status = ev.status;
    let pool = db.pool();
    db.block(async move {
        let mut tx = pool.begin().await.ok()?;
        let row: Option<(Uuid,)> = sqlx::query_as(
            "SELECT tenant_id FROM payment_subscriptions WHERE provider = $1 AND subscription_ref = $2",
        )
        .bind(&provider)
        .bind(&sub_ref)
        .fetch_optional(&mut *tx)
        .await
        .ok()?;
        let tenant_id = row?.0;
        sqlx::query(
            "UPDATE payment_subscriptions SET status = $3, updated_at = $4
             WHERE provider = $1 AND subscription_ref = $2",
        )
        .bind(&provider)
        .bind(&sub_ref)
        .bind(status)
        .bind(&now)
        .execute(&mut *tx)
        .await
        .ok()?;
        sqlx::query(
            "UPDATE tenant_subscriptions SET status = $2 WHERE tenant_id = $1 AND ended_at IS NULL",
        )
        .bind(tenant_id)
        .bind(status)
        .execute(&mut *tx)
        .await
        .ok()?;
        tx.commit().await.ok()?;
        Some(tenant_id)
    })
}
