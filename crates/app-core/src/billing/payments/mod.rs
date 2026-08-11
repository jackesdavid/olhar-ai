//! Payments — the **provider seam**. The billing core (plans / invoices / usage) is
//! provider-agnostic; only charge creation + webhook handling live behind
//! [`PaymentGateway`]. Pick one with `PAYMENT_PROVIDER`:
//!
//! - [`stripe`] — Brazil **and** the American market (multi-currency, recurring Pix
//!   Automático since 2026-04, card, boleto). Checkout-style: the subscription only exists
//!   *after* the customer pays, so its ref arrives on the webhook.
//! - [`asaas`] — Brazil only (cheaper Pix, native NF-e). Creates the subscription up front.
//!
//! Both shapes fit [`start_subscription`](PaymentGateway::start_subscription), which returns
//! whatever is known now ([`StartedSubscription`]). Webhooks are **idempotent**: providers
//! retry, so every event id is recorded in `payment_events` and applied at most once.

pub mod asaas;
pub mod stripe;

use std::collections::HashMap;

use uuid::Uuid;

use crate::db::Db;
use crate::timeutil::now_rfc3339;

/// The tier being subscribed to, priced in the tenant's currency.
pub struct PlanRef<'a> {
    pub plan_id: &'a str,
    pub name: &'a str,
    pub amount: f64,
    pub currency: &'a str,
    /// The provider's price-object id, when one was created before (Stripe `price_…`).
    pub price_ref: Option<&'a str>,
}

/// What is known right after starting a subscription.
pub struct StartedSubscription {
    /// Known immediately (Asaas). `None` for Checkout-style providers (Stripe) — the
    /// webhook supplies it once the customer actually pays.
    pub subscription_ref: Option<String>,
    /// Hosted page where the customer pays (Pix QR / boleto / card).
    pub payment_url: Option<String>,
    /// A provider price object we just created, for the caller to persist and reuse.
    pub price_ref: Option<String>,
}

/// A normalized webhook event.
pub struct WebhookEvent {
    /// Provider event id — the idempotency key (providers retry deliveries).
    pub event_id: String,
    pub kind: EventKind,
}

pub enum EventKind {
    /// An existing subscription changed status.
    StatusChanged {
        subscription_ref: String,
        /// `active` | `past_due` | `canceled`
        status: &'static str,
    },
    /// Checkout completed: the provider created the subscription — bind it to the tenant
    /// and move them onto the paid plan.
    SubscriptionStarted {
        subscription_ref: String,
        tenant_id: Uuid,
        plan_id: String,
        billing_type: String,
    },
}

/// A payment provider — wraps one gateway's REST API + webhook format.
pub trait PaymentGateway: Send + Sync {
    fn provider(&self) -> &'static str;

    /// The billing types this gateway can actually charge, most-preferred first. The UI renders
    /// exactly these and `subscribe` rejects anything else, so a method we cannot complete is
    /// never offered — the customer finds out at the button, not at the payment page.
    fn payment_methods(&self) -> &'static [&'static str];

    /// Ensure a provider-side customer exists; return its ref. `tax_id` is the Brazilian
    /// CPF/CNPJ (required by Asaas, optional for Stripe).
    fn ensure_customer(
        &self,
        name: &str,
        email: &str,
        tax_id: Option<&str>,
    ) -> Result<String, String>;

    /// Start a subscription: create it outright (Asaas) or open a hosted Checkout session
    /// (Stripe). `tenant_id` is echoed back by Checkout-style providers so their webhook
    /// can bind the resulting subscription to the right tenant.
    fn start_subscription(
        &self,
        customer_ref: &str,
        plan: &PlanRef<'_>,
        billing_type: &str,
        tenant_id: Uuid,
    ) -> Result<StartedSubscription, String>;

    /// A hosted page where the customer manages the subscription they already have — above all,
    /// updating the card behind a failed payment. `None` for a provider with no such page.
    ///
    /// This exists so that "my payment failed" has an answer that is not "subscribe again". It was
    /// added after a `past_due` tenant did exactly that and ended up with TWO live subscriptions on
    /// one customer, the old one still dunning a card that had already been replaced.
    fn manage_url(&self, _customer_ref: &str, _return_url: &str) -> Result<Option<String>, String> {
        Ok(None)
    }

    /// Cancel a subscription at the provider, immediately. Used to undo a duplicate, and by the
    /// tenant-facing cancel flow. Idempotent from the caller's side: cancelling something already
    /// cancelled must not be an error.
    fn cancel_subscription(&self, _subscription_ref: &str) -> Result<(), String> {
        Err("this provider cannot cancel a subscription from the API".into())
    }

    /// Authenticate an inbound webhook from its **raw body** + headers (lowercased keys).
    /// Stripe signs the payload (HMAC); Asaas sends a shared token.
    fn verify_webhook(&self, raw_body: &[u8], headers: &HashMap<String, String>) -> bool;

    /// Parse the raw body into a normalized event (`None` for events we ignore).
    fn parse_event(&self, raw_body: &[u8]) -> Option<WebhookEvent>;
}

/// The configured gateway (`PAYMENT_PROVIDER`, default `stripe`). `None` if unconfigured.
pub fn active_gateway() -> Option<Box<dyn PaymentGateway>> {
    let provider = std::env::var("PAYMENT_PROVIDER").unwrap_or_else(|_| "stripe".to_string());
    match provider.as_str() {
        "stripe" => {
            stripe::StripeGateway::from_env().map(|g| Box::new(g) as Box<dyn PaymentGateway>)
        }
        "asaas" => asaas::AsaasGateway::from_env().map(|g| Box::new(g) as Box<dyn PaymentGateway>),
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

/// Record a provider subscription mapping. `status` starts `pending` until a payment
/// confirms it (Asaas), or `active` when bound from a completed Checkout (Stripe).
pub fn record_subscription(
    db: &Db,
    tenant_id: Uuid,
    provider: &str,
    subscription_ref: &str,
    plan_id: &str,
    billing_type: &str,
    status: &str,
) {
    if lookup(db, provider, subscription_ref).is_some() {
        return; // already mapped (a retried webhook)
    }
    let id = Uuid::new_v4();
    let now = now_rfc3339();
    let provider = provider.to_string();
    let subscription_ref = subscription_ref.to_string();
    let plan_id = plan_id.to_string();
    let billing_type = billing_type.to_string();
    let status = status.to_string();
    let pool = db.pool();
    let res: Result<_, sqlx::Error> = db.block(async move {
        sqlx::query(
            "INSERT INTO payment_subscriptions
                (id, tenant_id, provider, subscription_ref, plan_id, billing_type, status, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $8)",
        )
        .bind(id)
        .bind(tenant_id)
        .bind(&provider)
        .bind(&subscription_ref)
        .bind(&plan_id)
        .bind(&billing_type)
        .bind(&status)
        .bind(&now)
        .execute(&pool)
        .await
    });
    if let Err(e) = res {
        log::error!("payments::record_subscription failed: {e}");
    }
}

/// `(tenant_id, plan_id)` behind a provider subscription ref.
fn lookup(db: &Db, provider: &str, subscription_ref: &str) -> Option<(Uuid, String)> {
    let provider = provider.to_string();
    let subscription_ref = subscription_ref.to_string();
    let pool = db.pool();
    db.block(async move {
        sqlx::query_as::<_, (Uuid, String)>(
            "SELECT tenant_id, plan_id FROM payment_subscriptions
             WHERE provider = $1 AND subscription_ref = $2",
        )
        .bind(&provider)
        .bind(&subscription_ref)
        .fetch_optional(&pool)
        .await
        .ok()
        .flatten()
    })
}

/// Idempotency guard: returns `true` the **first** time we see `event_id`. Providers retry
/// deliveries, and a duplicate must not be applied twice. On a DB error we return `true`
/// (process it) — our status writes are idempotent, so a re-apply is harmless; dropping an
/// event is not.
pub fn mark_event_seen(db: &Db, provider: &str, event_id: &str) -> bool {
    let now = now_rfc3339();
    let provider = provider.to_string();
    let event_id = event_id.to_string();
    let pool = db.pool();
    let res: Result<u64, sqlx::Error> = db.block(async move {
        sqlx::query(
            "INSERT INTO payment_events (provider, event_id, received_at) VALUES ($1, $2, $3)
             ON CONFLICT (provider, event_id) DO NOTHING",
        )
        .bind(&provider)
        .bind(&event_id)
        .bind(&now)
        .execute(&pool)
        .await
        .map(|r| r.rows_affected())
    });
    match res {
        Ok(1) => true,
        Ok(_) => false, // already handled
        Err(e) => {
            log::error!("payments::mark_event_seen failed: {e}");
            true
        }
    }
}

/// Apply a status change to a known subscription: update the provider mapping and the
/// tenant's billing subscription (which gates access). On `active`, make sure the tenant is
/// actually on the paid plan. Returns the affected tenant.
pub fn apply_status(db: &Db, provider: &str, subscription_ref: &str, status: &str) -> Option<Uuid> {
    let (tenant_id, plan_id) = lookup(db, provider, subscription_ref)?;
    set_provider_status(db, provider, subscription_ref, status);

    if status == "active" {
        let already = super::subscriptions::current(db, tenant_id)
            .map(|s| s.plan_id == plan_id && s.status == "active")
            .unwrap_or(false);
        if !already {
            if let Err(e) = super::subscriptions::set_plan(db, tenant_id, &plan_id) {
                log::error!("payments: set_plan after payment failed: {e}");
            }
        }
        return Some(tenant_id);
    }

    // A DOWNGRADE, and it must only come from the subscription the tenant is actually living on.
    //
    // This check is the whole fix for a real incident: a tenant held two provider subscriptions —
    // an old `past_due` one and the new `active` one they had just paid — and cancelling the dead
    // one flipped the tenant to `canceled`, because this function used to write the current
    // `tenant_subscriptions` row for ANY non-active event, whatever it came from. Uploads stopped
    // for a customer who was paid up. Any stale subscription left at the provider could revoke a
    // live one at any moment.
    if let Some(other) = live_subscription_excluding(db, tenant_id, provider, subscription_ref) {
        log::info!(
            "payments: ignoring '{status}' for {provider}:{subscription_ref} — tenant {tenant_id} \
             still has a live subscription ({other})"
        );
        return Some(tenant_id);
    }
    set_tenant_status(db, tenant_id, status);
    Some(tenant_id)
}

/// Statuses that mean "this provider subscription is still carrying the tenant".
const LIVE_STATUSES: [&str; 2] = ["active", "trialing"];

/// Another provider subscription for this tenant that is still live, if any — the guard that keeps
/// a stale subscription's death from taking a paying tenant with it.
fn live_subscription_excluding(
    db: &Db,
    tenant_id: Uuid,
    provider: &str,
    subscription_ref: &str,
) -> Option<String> {
    let provider = provider.to_string();
    let subscription_ref = subscription_ref.to_string();
    let pool = db.pool();
    db.block(async move {
        sqlx::query_scalar::<_, String>(
            "SELECT subscription_ref FROM payment_subscriptions
              WHERE tenant_id = $1
                AND NOT (provider = $2 AND subscription_ref = $3)
                AND status = ANY($4)
              ORDER BY updated_at DESC
              LIMIT 1",
        )
        .bind(tenant_id)
        .bind(&provider)
        .bind(&subscription_ref)
        .bind(LIVE_STATUSES.map(|s| s.to_string()).to_vec())
        .fetch_optional(&pool)
        .await
        .unwrap_or_else(|e| {
            // Fail CLOSED on the guard, not on the tenant: if we cannot prove another subscription
            // is alive we fall through to the downgrade, which is the pre-existing behaviour.
            log::error!("payments::live_subscription_excluding failed: {e}");
            None
        })
    })
}

/// The tenant's live provider subscription, if it has one. `subscribe` refuses to open a second.
pub fn live_subscription(db: &Db, tenant_id: Uuid) -> Option<(String, String, String)> {
    let pool = db.pool();
    db.block(async move {
        sqlx::query_as::<_, (String, String, String)>(
            "SELECT provider, subscription_ref, status FROM payment_subscriptions
              WHERE tenant_id = $1 AND status = ANY($2)
              ORDER BY updated_at DESC
              LIMIT 1",
        )
        .bind(tenant_id)
        .bind(
            ["active", "trialing", "past_due", "pending"]
                .map(|s| s.to_string())
                .to_vec(),
        )
        .fetch_optional(&pool)
        .await
        .unwrap_or_else(|e| {
            log::error!("payments::live_subscription failed: {e}");
            None
        })
    })
}

/// Bind a Checkout-created subscription to its tenant, then activate the plan.
pub fn bind_subscription(
    db: &Db,
    tenant_id: Uuid,
    provider: &str,
    subscription_ref: &str,
    plan_id: &str,
    billing_type: &str,
) {
    record_subscription(
        db,
        tenant_id,
        provider,
        subscription_ref,
        plan_id,
        billing_type,
        "active",
    );
    apply_status(db, provider, subscription_ref, "active");
}

fn set_provider_status(db: &Db, provider: &str, subscription_ref: &str, status: &str) {
    let now = now_rfc3339();
    let provider = provider.to_string();
    let subscription_ref = subscription_ref.to_string();
    let status = status.to_string();
    let pool = db.pool();
    let res: Result<_, sqlx::Error> = db.block(async move {
        sqlx::query(
            "UPDATE payment_subscriptions SET status = $3, updated_at = $4
             WHERE provider = $1 AND subscription_ref = $2",
        )
        .bind(&provider)
        .bind(&subscription_ref)
        .bind(&status)
        .bind(&now)
        .execute(&pool)
        .await
    });
    if let Err(e) = res {
        log::error!("payments::set_provider_status failed: {e}");
    }
}

fn set_tenant_status(db: &Db, tenant_id: Uuid, status: &str) {
    let status = status.to_string();
    let pool = db.pool();
    let res: Result<_, sqlx::Error> = db.block(async move {
        sqlx::query(
            "UPDATE tenant_subscriptions SET status = $2 WHERE tenant_id = $1 AND ended_at IS NULL",
        )
        .bind(tenant_id)
        .bind(&status)
        .execute(&pool)
        .await
    });
    if let Err(e) = res {
        log::error!("payments::set_tenant_status failed: {e}");
    }
}
