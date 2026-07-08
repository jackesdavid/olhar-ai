//! Billing API. Tenant self-serve (guarded by `billing.read`): plans, current
//! subscription, invoices, and a running-period preview. Master cross-tenant (guarded by
//! the `master` claim): list a period's invoices, close a period into an immutable
//! invoice, and grant/reset a Beta trial.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::Json;
use axum::routing::{get, post};
use axum::Router;
use serde::Deserialize;
use uuid::Uuid;

use app_core::billing::{invoices, plans, subscriptions};

use super::{blocking, err, tenant_of, ApiErr};
use crate::auth::{self, UserClaims};
use crate::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/billing/plans", get(list_plans))
        .route("/billing/subscription", get(my_subscription))
        .route("/billing/invoices", get(my_invoices))
        .route("/billing/invoices/:period", get(my_invoice))
        .route("/admin/billing", get(admin_list))
        .route("/admin/billing/:tenant/close", post(admin_close))
        .route("/admin/billing/:tenant/trial", post(admin_grant_trial))
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
