//! Billing API. Tenant self-serve (guarded by `billing.read`): plans, current
//! subscription, invoices, and a running-period preview. Master cross-tenant (guarded by
//! the `master` claim): list a period's invoices, close a period into an immutable
//! invoice, and grant/reset a Beta trial.

use std::collections::HashMap;

use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Json;
use axum::routing::{get, post};
use axum::Router;
use serde::Deserialize;
use uuid::Uuid;

use app_core::billing::{self, invoices, payments, plans, storage, subscriptions};

use super::{blocking, err, tenant_of, ApiErr};
use crate::auth::{self, UserClaims};
use crate::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/billing/plans", get(list_plans))
        .route("/billing/payment-methods", get(payment_methods))
        .route("/billing/storage", get(my_storage))
        .route("/billing/subscription", get(my_subscription))
        .route("/billing/invoices", get(my_invoices))
        .route("/billing/invoices/:period", get(my_invoice))
        .route("/billing/subscribe", post(subscribe))
        .route("/billing/portal", post(portal))
        .route("/admin/billing", get(admin_list))
        .route("/admin/billing/overview", get(admin_overview))
        .route("/admin/storage", get(admin_storage))
        .route("/admin/storage/:tenant/purge", post(admin_purge_storage))
        .route("/admin/billing/:tenant/close", post(admin_close))
        .route("/admin/billing/:tenant/trial", post(admin_grant_trial))
        .route("/admin/billing/:tenant/plan", post(admin_set_plan))
        .route(
            "/admin/plans",
            get(admin_list_plans).post(admin_upsert_plan),
        )
        .route("/admin/plans/:id/price", post(admin_set_price))
        // Public: authenticated by the provider (Stripe HMAC / Asaas token), not a JWT.
        .route("/webhooks/stripe", post(payments_webhook))
        .route("/webhooks/asaas", post(payments_webhook))
}

/// The current calendar month as `YYYY-MM` (the default billing period).
fn current_period() -> String {
    chrono::Local::now().format("%Y-%m").to_string()
}

#[derive(Deserialize)]
struct PeriodQuery {
    period: Option<String>,
}

/// Master-only guard (master's token carries no tenant, so it bypasses `tenant_of`).
fn require_master(c: &crate::auth::Claims) -> Result<(), ApiErr> {
    if c.master {
        Ok(())
    } else {
        Err(err(StatusCode::FORBIDDEN, "master only"))
    }
}

// ---- tenant self-serve (billing.read) ----

/// Active tiers priced in the tenant's own currency (BRL or USD).
async fn list_plans(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
) -> Result<Json<Vec<plans::PricedPlan>>, ApiErr> {
    auth::require(&c, "billing.read")?;
    let tid = tenant_of(&c)?;
    let db = st.db.clone();
    Ok(Json(
        blocking(move || {
            let currency = billing::tenant_currency(&db, tid);
            plans::list_active_priced(&db, &currency)
        })
        .await?,
    ))
}

/// The billing types the configured gateway can actually charge. Empty when no provider is
/// configured, which the UI reads as "subscribing is unavailable" rather than offering buttons
/// that cannot work.
async fn payment_methods(UserClaims(c): UserClaims) -> Result<Json<Vec<&'static str>>, ApiErr> {
    auth::require(&c, "billing.read")?;
    let methods = payments::active_gateway()
        .map(|gw| gw.payment_methods().to_vec())
        .unwrap_or_default();
    Ok(Json(methods))
}

/// Enrich a storage row with the server-computed overage so the UI never recomputes money.
fn storage_json(s: &storage::TenantStorage) -> serde_json::Value {
    serde_json::json!({
        "tenant_id": s.tenant_id,
        "tenant_name": s.tenant_name,
        "currency": s.currency,
        "plan_id": s.plan_id,
        "plan_name": s.plan_name,
        "retention_days": s.retention_days,
        "included_storage_gb": s.included_storage_gb,
        "overage_per_gb": s.overage_per_gb,
        "stored_bytes": s.stored_bytes,
        "clip_count": s.clip_count,
        "gb_month": s.gb_month,
        "projected_overage": s.projected_overage(),
    })
}

/// This tenant's own storage: durable footage vs quota, retention, projected overage.
async fn my_storage(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiErr> {
    auth::require(&c, "billing.read")?;
    let tid = tenant_of(&c)?;
    let db = st.db.clone();
    let row = blocking(move || storage::for_tenant(&db, tid)).await?;
    Ok(Json(
        row.map(|s| storage_json(&s))
            .unwrap_or(serde_json::Value::Null),
    ))
}

/// Every tenant's storage, heaviest first — the master's view to catch a runaway recorder.
async fn admin_storage(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
) -> Result<Json<Vec<serde_json::Value>>, ApiErr> {
    require_master(&c)?;
    let db = st.db.clone();
    let rows = blocking(move || storage::overview(&db)).await?;
    Ok(Json(rows.iter().map(storage_json).collect()))
}

/// Purge a tenant's cloud footage now — delete every R2 object under `*/{tenant}/` and detach the
/// rows. For a tenant leaving for good or a data-subject deletion; not run on mere cancellation
/// (the lifecycle already expires a lapsed tenant's clips). Audited to `platform_logs`.
async fn admin_purge_storage(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
    Path(tenant): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiErr> {
    require_master(&c)?;
    let db = st.db.clone();
    // Purge across every retention tier the catalog defines — a tenant may have clips under a
    // prefix from an older plan, not just their current one.
    let tiers = blocking({
        let db = db.clone();
        move || plans::retention_tiers(&db)
    })
    .await?;
    let removed =
        tokio::task::spawn_blocking(move || app_core::clip_store::purge_tenant(tenant, &tiers))
            .await
            .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, format!("task: {e}")))?
            .map_err(|e| {
                log::error!("purge tenant {tenant} storage: {e}");
                err(StatusCode::BAD_GATEWAY, "purge failed")
            })?;
    let cleared = crate::store::clear_tenant_cloud_keys(&st.pool, tenant)
        .await
        .map_err(|e| {
            log::error!("clear cloud keys {tenant}: {e}");
            err(StatusCode::INTERNAL_SERVER_ERROR, "database error")
        })?;

    // Audit: who purged what, when. This is the record a data-subject deletion is answered with.
    st.log_queue.push(app_core::platform_logs::PlatformLog {
        tenant_id: tenant,
        module: "billing",
        trace_id: None,
        entity_id: Some(tenant.to_string()),
        entity_type: Some("tenant"),
        level: "warn",
        message: format!(
            "cloud storage purged: {removed} object(s) removed, {cleared} clip(s) detached"
        ),
        payload: Some(
            serde_json::json!({ "objects_removed": removed, "rows_cleared": cleared, "by": c.sub }),
        ),
    });

    Ok(Json(
        serde_json::json!({ "objects_removed": removed, "rows_cleared": cleared }),
    ))
}

async fn my_subscription(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
) -> Result<Json<Option<subscriptions::Subscription>>, ApiErr> {
    auth::require(&c, "billing.read")?;
    let tid = tenant_of(&c)?;
    let db = st.db.clone();
    Ok(Json(
        blocking(move || subscriptions::current(&db, tid)).await?,
    ))
}

async fn my_invoices(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
) -> Result<Json<Vec<invoices::Invoice>>, ApiErr> {
    auth::require(&c, "billing.read")?;
    let tid = tenant_of(&c)?;
    let db = st.db.clone();
    Ok(Json(
        blocking(move || invoices::list_for_tenant(&db, tid)).await?,
    ))
}

/// One period: the stored invoice + its line items if closed, else a computed draft.
async fn my_invoice(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
    Path(period): Path<String>,
) -> Result<Json<serde_json::Value>, ApiErr> {
    auth::require(&c, "billing.read")?;
    let tid = tenant_of(&c)?;
    let db = st.db.clone();
    let out = blocking(move || match invoices::get(&db, tid, &period) {
        Some(inv) => {
            let items = invoices::line_items(&db, inv.id);
            serde_json::json!({ "invoice": inv, "line_items": items, "draft": null })
        }
        None => serde_json::json!({ "invoice": null, "line_items": [], "draft": invoices::compute(&db, tid, &period) }),
    })
    .await?;
    Ok(Json(out))
}

// ---- master cross-tenant ----

async fn admin_list(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
    Query(q): Query<PeriodQuery>,
) -> Result<Json<Vec<invoices::Invoice>>, ApiErr> {
    require_master(&c)?;
    let period = q.period.unwrap_or_else(current_period);
    let db = st.db.clone();
    Ok(Json(
        blocking(move || invoices::list_for_period(&db, &period)).await?,
    ))
}

async fn admin_close(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
    Path(tenant): Path<Uuid>,
    Query(q): Query<PeriodQuery>,
) -> Result<Json<invoices::Invoice>, ApiErr> {
    require_master(&c)?;
    let period = q.period.unwrap_or_else(current_period);
    let db = st.db.clone();
    let inv = blocking(move || invoices::close_period(&db, tenant, &period))
        .await?
        .map_err(|e| err(StatusCode::BAD_REQUEST, e))?;
    Ok(Json(inv))
}

async fn admin_grant_trial(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
    Path(tenant): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiErr> {
    require_master(&c)?;
    let db = st.db.clone();
    let id = blocking(move || subscriptions::start_trial(&db, tenant))
        .await?
        .map_err(|e| err(StatusCode::BAD_REQUEST, e))?;
    Ok(Json(serde_json::json!({ "subscription_id": id })))
}

/// Every tenant with its current plan + status.
async fn admin_overview(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
) -> Result<Json<Vec<subscriptions::TenantBilling>>, ApiErr> {
    require_master(&c)?;
    let db = st.db.clone();
    Ok(Json(blocking(move || subscriptions::overview(&db)).await?))
}

#[derive(Deserialize)]
struct SetPlanReq {
    plan_id: String,
}

/// Master override: put a tenant onto a plan directly (no payment).
async fn admin_set_plan(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
    Path(tenant): Path<Uuid>,
    Json(req): Json<SetPlanReq>,
) -> Result<Json<serde_json::Value>, ApiErr> {
    require_master(&c)?;
    let db = st.db.clone();
    let id = blocking(move || subscriptions::set_plan(&db, tenant, &req.plan_id))
        .await?
        .map_err(|e| err(StatusCode::BAD_REQUEST, e))?;
    Ok(Json(serde_json::json!({ "subscription_id": id })))
}

/// The full plan catalog (active + inactive), each tier with its per-currency prices.
async fn admin_list_plans(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
) -> Result<Json<Vec<plans::PlanWithPrices>>, ApiErr> {
    require_master(&c)?;
    let db = st.db.clone();
    Ok(Json(
        blocking(move || plans::list_all_with_prices(&db)).await?,
    ))
}

/// Create or edit a tier (cap, trial, quotas — money lives in prices).
async fn admin_upsert_plan(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
    Json(plan): Json<plans::Plan>,
) -> Result<StatusCode, ApiErr> {
    require_master(&c)?;
    let db = st.db.clone();
    blocking(move || plans::upsert(&db, plan))
        .await?
        .map_err(|e| err(StatusCode::BAD_REQUEST, e))?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct SetPriceReq {
    currency: String,
    amount: f64,
    /// Storage overage, per GB-month, in this currency. Omitted = free (the Beta trial).
    #[serde(default)]
    overage_storage_per_gb: f64,
}

/// Set a tier's money in one currency: base fee + storage overage rate. Quotas live on the
/// tier (`POST /admin/plans`) — a GB is a GB in any currency, but a rate is money.
async fn admin_set_price(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<SetPriceReq>,
) -> Result<StatusCode, ApiErr> {
    require_master(&c)?;
    let db = st.db.clone();
    blocking(move || {
        plans::set_price(
            &db,
            &id,
            &req.currency.to_uppercase(),
            req.amount,
            req.overage_storage_per_gb,
        )
    })
    .await?
    .map_err(|e| err(StatusCode::BAD_REQUEST, e))?;
    Ok(StatusCode::NO_CONTENT)
}

// ---- payments ----

/// A hosted page where the customer manages the subscription they already have — updating the card
/// behind a failed payment above all.
///
/// This is the CORRECT action for `past_due`, and it exists because the product previously had
/// none: the only button was Subscribe, so a customer whose card failed created a second
/// subscription while the first kept dunning. Read-only permission on purpose — `billing.read` is
/// the right gate for "let me fix my own payment", and requiring `billing.write` would lock a
/// tenant out of paying us.
async fn portal(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiErr> {
    auth::require(&c, "billing.read")?;
    let tid = tenant_of(&c)?;
    let db = st.db.clone();
    let url = blocking(move || {
        let gw = payments::active_gateway().ok_or("no payment provider configured")?;
        let customer =
            payments::customer_ref(&db, tid, gw.provider()).ok_or("no customer with the provider")?;
        let return_url = std::env::var("APP_ORIGIN")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| "https://app.olharai.com".to_string());
        gw.manage_url(&customer, &format!("{}/billing", return_url.trim_end_matches('/')))?
            .ok_or_else(|| "this payment provider has no self-service portal".to_string())
    })
    .await?
    .map_err(|e| err(StatusCode::BAD_GATEWAY, e))?;
    Ok(Json(serde_json::json!({ "url": url })))
}

#[derive(Deserialize)]
struct SubscribeReq {
    plan_id: String,
    billing_type: String, // 'PIX' | 'BOLETO' | 'CREDIT_CARD'
    name: String,
    email: String,
    cpf_cnpj: Option<String>,
}

/// Start a paid subscription: ensure the provider customer, then create the subscription
/// (Asaas) or a hosted Checkout session (Stripe). The tenant is moved onto the paid plan
/// only when the provider's webhook confirms payment — never optimistically here.
async fn subscribe(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
    Json(req): Json<SubscribeReq>,
) -> Result<Json<serde_json::Value>, ApiErr> {
    auth::require(&c, "billing.write")?;
    let tid = tenant_of(&c)?;
    let db = st.db.clone();
    let started = blocking(move || {
        let gw = payments::active_gateway().ok_or("no payment provider configured")?;
        let provider = gw.provider();
        // ONE subscription per tenant. Without this a `past_due` customer doing the obvious thing —
        // pressing Subscribe again, which the plan grid happily offers because `isCurrent` only
        // recognises `active` — ends up with two live subscriptions on one provider customer, the
        // old one still dunning a card they have already replaced. That happened; this is the guard.
        // Fixing a failed payment is `POST /billing/portal`, not a second subscription.
        if let Some((_, sub_ref, status)) = payments::live_subscription(&db, tid) {
            return Err(format!(
                "this workspace already has a subscription ({sub_ref}, {status}) — \
                 manage it instead of starting another"
            ));
        }
        // The client picks the method, so trust it as far as the gateway does. Without this a
        // stale UI (or a hand-rolled request) reaches the provider and comes back with its error.
        if !gw.payment_methods().contains(&req.billing_type.as_str()) {
            return Err(format!(
                "{provider} does not accept {} — available: {}",
                req.billing_type,
                gw.payment_methods().join(", ")
            ));
        }
        let plan = plans::get(&db, &req.plan_id).ok_or("plan not found")?;
        let currency = billing::tenant_currency(&db, tid);
        let amount = plans::price(&db, &plan.id, &currency)
            .ok_or_else(|| format!("plan '{}' has no price in {currency}", plan.id))?;

        let customer_ref = match payments::customer_ref(&db, tid, provider) {
            Some(r) => r,
            None => {
                let r = gw.ensure_customer(&req.name, &req.email, req.cpf_cnpj.as_deref())?;
                payments::upsert_customer(&db, tid, provider, &r);
                r
            }
        };

        let price_ref = plans::provider_price_ref(&db, &plan.id, provider, &currency);
        let plan_ref = payments::PlanRef {
            plan_id: &plan.id,
            name: &plan.name,
            amount,
            currency: &currency,
            price_ref: price_ref.as_deref(),
        };
        let started = gw.start_subscription(&customer_ref, &plan_ref, &req.billing_type, tid)?;

        // Persist a freshly created provider price object so we reuse it next time — for this
        // currency only. The Price the provider just minted carries `currency`; reusing it for
        // another one charges that tenant in the wrong money.
        if let Some(p) = &started.price_ref {
            let _ = plans::set_provider_price_ref(&db, &plan.id, provider, &currency, p);
        }
        // Asaas knows the subscription now; park it as `pending` until payment confirms.
        if let Some(sub_ref) = &started.subscription_ref {
            payments::record_subscription(
                &db,
                tid,
                provider,
                sub_ref,
                &plan.id,
                &req.billing_type,
                "pending",
            );
        }
        Ok::<_, String>(started)
    })
    .await?
    .map_err(|e| err(StatusCode::BAD_REQUEST, e))?;

    Ok(Json(serde_json::json!({
        "subscription_ref": started.subscription_ref,
        "payment_url": started.payment_url,
    })))
}

/// Payment-provider webhook (public — authenticated by the provider's own scheme, not a
/// JWT): Stripe signs the raw body (HMAC), Asaas sends a shared token. Events are applied
/// **at most once** (`payment_events` idempotency) — providers retry deliveries.
async fn payments_webhook(
    State(st): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<StatusCode, ApiErr> {
    // Lowercased header map — the gateway looks up whatever it signs with.
    let hdrs: HashMap<String, String> = headers
        .iter()
        .filter_map(|(k, v)| {
            v.to_str()
                .ok()
                .map(|v| (k.as_str().to_ascii_lowercase(), v.to_string()))
        })
        .collect();
    let db = st.db.clone();
    blocking(move || {
        let gw = payments::active_gateway().ok_or("no payment provider configured")?;
        if !gw.verify_webhook(&body, &hdrs) {
            return Err("invalid webhook signature".to_string());
        }
        let Some(ev) = gw.parse_event(&body) else {
            return Ok(()); // an event we don't care about
        };
        if !payments::mark_event_seen(&db, gw.provider(), &ev.event_id) {
            log::info!("payments: duplicate webhook {} ignored", ev.event_id);
            return Ok(());
        }
        match ev.kind {
            payments::EventKind::StatusChanged {
                subscription_ref,
                status,
            } => {
                payments::apply_status(&db, gw.provider(), &subscription_ref, status);
            }
            payments::EventKind::SubscriptionStarted {
                subscription_ref,
                tenant_id,
                plan_id,
                billing_type,
            } => {
                payments::bind_subscription(
                    &db,
                    tenant_id,
                    gw.provider(),
                    &subscription_ref,
                    &plan_id,
                    &billing_type,
                );
            }
        }
        Ok::<_, String>(())
    })
    .await?
    .map_err(|e| err(StatusCode::UNAUTHORIZED, e))?;
    Ok(StatusCode::OK)
}
