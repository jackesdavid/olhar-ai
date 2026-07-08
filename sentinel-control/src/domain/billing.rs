//! Billing API. Tenant self-serve (guarded by `billing.read`): plans, current
//! subscription, invoices, and a running-period preview. Master cross-tenant (guarded by
//! the `master` claim): list a period's invoices, close a period into an immutable
//! invoice, and grant/reset a Beta trial.

use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Json;
use axum::routing::{get, post};
use axum::Router;
use serde::Deserialize;
use uuid::Uuid;

use app_core::billing::{invoices, payments, plans, subscriptions};

use super::{blocking, err, tenant_of, ApiErr};
use crate::auth::{self, UserClaims};
use crate::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/billing/plans", get(list_plans))
        .route("/billing/subscription", get(my_subscription))
        .route("/billing/invoices", get(my_invoices))
        .route("/billing/invoices/:period", get(my_invoice))
        .route("/billing/subscribe", post(subscribe))
        .route("/admin/billing", get(admin_list))
        .route("/admin/billing/overview", get(admin_overview))
        .route("/admin/billing/:tenant/close", post(admin_close))
        .route("/admin/billing/:tenant/trial", post(admin_grant_trial))
        .route("/admin/billing/:tenant/plan", post(admin_set_plan))
        .route("/admin/plans", get(admin_list_plans).post(admin_upsert_plan))
        // Public: verified by the Asaas access-token header, not a JWT.
        .route("/webhooks/asaas", post(asaas_webhook))
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

async fn list_plans(UserClaims(c): UserClaims, State(st): State<AppState>) -> Result<Json<Vec<plans::Plan>>, ApiErr> {
    auth::require(&c, "billing.read")?;
    let db = st.db.clone();
    Ok(Json(blocking(move || plans::list_active(&db)).await?))
}

async fn my_subscription(UserClaims(c): UserClaims, State(st): State<AppState>) -> Result<Json<Option<subscriptions::Subscription>>, ApiErr> {
    auth::require(&c, "billing.read")?;
    let tid = tenant_of(&c)?;
    let db = st.db.clone();
    Ok(Json(blocking(move || subscriptions::current(&db, tid)).await?))
}

async fn my_invoices(UserClaims(c): UserClaims, State(st): State<AppState>) -> Result<Json<Vec<invoices::Invoice>>, ApiErr> {
    auth::require(&c, "billing.read")?;
    let tid = tenant_of(&c)?;
    let db = st.db.clone();
    Ok(Json(blocking(move || invoices::list_for_tenant(&db, tid)).await?))
}

/// One period: the stored invoice + its line items if closed, else a computed draft.
async fn my_invoice(UserClaims(c): UserClaims, State(st): State<AppState>, Path(period): Path<String>) -> Result<Json<serde_json::Value>, ApiErr> {
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

async fn admin_list(UserClaims(c): UserClaims, State(st): State<AppState>, Query(q): Query<PeriodQuery>) -> Result<Json<Vec<invoices::Invoice>>, ApiErr> {
    require_master(&c)?;
    let period = q.period.unwrap_or_else(current_period);
    let db = st.db.clone();
    Ok(Json(blocking(move || invoices::list_for_period(&db, &period)).await?))
}

async fn admin_close(UserClaims(c): UserClaims, State(st): State<AppState>, Path(tenant): Path<Uuid>, Query(q): Query<PeriodQuery>) -> Result<Json<invoices::Invoice>, ApiErr> {
    require_master(&c)?;
    let period = q.period.unwrap_or_else(current_period);
    let db = st.db.clone();
    let inv = blocking(move || invoices::close_period(&db, tenant, &period))
        .await?
        .map_err(|e| err(StatusCode::BAD_REQUEST, e))?;
    Ok(Json(inv))
}

async fn admin_grant_trial(UserClaims(c): UserClaims, State(st): State<AppState>, Path(tenant): Path<Uuid>) -> Result<Json<serde_json::Value>, ApiErr> {
    require_master(&c)?;
    let db = st.db.clone();
    let id = blocking(move || subscriptions::start_trial(&db, tenant))
        .await?
        .map_err(|e| err(StatusCode::BAD_REQUEST, e))?;
    Ok(Json(serde_json::json!({ "subscription_id": id })))
}

/// Every tenant with its current plan + status.
async fn admin_overview(UserClaims(c): UserClaims, State(st): State<AppState>) -> Result<Json<Vec<subscriptions::TenantBilling>>, ApiErr> {
    require_master(&c)?;
    let db = st.db.clone();
    Ok(Json(blocking(move || subscriptions::overview(&db)).await?))
}

#[derive(Deserialize)]
struct SetPlanReq {
    plan_id: String,
}

/// Master override: put a tenant onto a plan directly (no payment).
async fn admin_set_plan(UserClaims(c): UserClaims, State(st): State<AppState>, Path(tenant): Path<Uuid>, Json(req): Json<SetPlanReq>) -> Result<Json<serde_json::Value>, ApiErr> {
    require_master(&c)?;
    let db = st.db.clone();
    let id = blocking(move || subscriptions::set_plan(&db, tenant, &req.plan_id))
        .await?
        .map_err(|e| err(StatusCode::BAD_REQUEST, e))?;
    Ok(Json(serde_json::json!({ "subscription_id": id })))
}

/// The full plan catalog (active + inactive).
async fn admin_list_plans(UserClaims(c): UserClaims, State(st): State<AppState>) -> Result<Json<Vec<plans::Plan>>, ApiErr> {
    require_master(&c)?;
    let db = st.db.clone();
    Ok(Json(blocking(move || plans::list_all(&db)).await?))
}

/// Create or edit a plan (price, cap, trial, quotas).
async fn admin_upsert_plan(UserClaims(c): UserClaims, State(st): State<AppState>, Json(plan): Json<plans::Plan>) -> Result<StatusCode, ApiErr> {
    require_master(&c)?;
    let db = st.db.clone();
    blocking(move || plans::upsert(&db, plan))
        .await?
        .map_err(|e| err(StatusCode::BAD_REQUEST, e))?;
    Ok(StatusCode::NO_CONTENT)
}

// ---- payments (Asaas) ----

#[derive(Deserialize)]
struct SubscribeReq {
    plan_id: String,
    billing_type: String, // 'PIX' | 'BOLETO' | 'CREDIT_CARD'
    name: String,
    email: String,
    cpf_cnpj: Option<String>,
}

/// Convert the tenant onto a paid plan: create the provider customer + subscription, mirror
/// the refs, and flip the billing subscription to the active plan.
async fn subscribe(UserClaims(c): UserClaims, State(st): State<AppState>, Json(req): Json<SubscribeReq>) -> Result<Json<serde_json::Value>, ApiErr> {
    auth::require(&c, "billing.write")?;
    let tid = tenant_of(&c)?;
    let db = st.db.clone();
    let sub_ref = blocking(move || {
        let gw = payments::active_gateway().ok_or("no payment provider configured")?;
        let plan = plans::get(&db, &req.plan_id).ok_or("plan not found")?;
        let customer_ref = match payments::customer_ref(&db, tid, gw.provider()) {
            Some(r) => r,
            None => {
                let r = gw.ensure_customer(&req.name, &req.email, req.cpf_cnpj.as_deref())?;
                payments::upsert_customer(&db, tid, gw.provider(), &r);
                r
            }
        };
        let sub_ref = gw.create_subscription(&customer_ref, plan.base_fee_brl, &req.billing_type, &plan.name)?;
        payments::record_subscription(&db, tid, gw.provider(), &sub_ref, &plan.id, &req.billing_type);
        subscriptions::set_plan(&db, tid, &plan.id)?;
        Ok::<_, String>(sub_ref)
    })
    .await?
    .map_err(|e| err(StatusCode::BAD_REQUEST, e))?;
    Ok(Json(serde_json::json!({ "subscription_ref": sub_ref })))
}

/// Asaas webhook: verify the access-token header, then apply the event to the tenant's
/// subscription status (which gates access). Public — no JWT.
async fn asaas_webhook(State(st): State<AppState>, headers: HeaderMap, body: Bytes) -> Result<StatusCode, ApiErr> {
    let token = headers.get("asaas-access-token").and_then(|v| v.to_str().ok()).map(String::from);
    let json: serde_json::Value =
        serde_json::from_slice(&body).map_err(|e| err(StatusCode::BAD_REQUEST, format!("bad json: {e}")))?;
    let db = st.db.clone();
    blocking(move || {
        let gw = payments::active_gateway().ok_or("no payment provider configured")?;
        if !gw.verify_webhook(token.as_deref()) {
            return Err("invalid webhook token".to_string());
        }
        if let Some(ev) = gw.parse_event(&json) {
            payments::apply_event(&db, gw.provider(), &ev);
        }
        Ok::<_, String>(())
    })
    .await?
    .map_err(|e| err(StatusCode::UNAUTHORIZED, e))?;
    Ok(StatusCode::OK)
}
