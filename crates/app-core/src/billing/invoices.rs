//! Customer invoices: aggregate a period's usage, apply the tenant's plan, and (on close)
//! persist an immutable `final` invoice with its line items. v1 tiers are flat, so the
//! total is the base fee; metered lines are shown with amount 0 for transparency.
//!
//! Periods are `'YYYY-MM'` and matched by RFC3339 **prefix** (`created_at LIKE '2026-07%'`),
//! which is offset-agnostic — no fragile timestamp arithmetic.

use sqlx::FromRow;
use uuid::Uuid;

use crate::alerts::now_rfc3339;
use crate::db::Db;

use super::{fx, plans, subscriptions};

/// Raw metered usage for one tenant over one period.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct TenantUsage {
    pub gpu_sec: f64,
    /// GPU cost in the AI rate's currency (treated as USD; see `AI_RATE_PER_SEC`).
    pub gpu_cost_usd: f64,
    pub storage_gb: f64,
    pub notifications: i64,
}

/// A computed (not-yet-persisted) invoice — used both for a running-period preview and as
/// the source for [`close_period`].
#[derive(Debug, Clone, serde::Serialize)]
pub struct InvoiceDraft {
    pub tenant_id: Uuid,
    pub period: String,
    pub plan_id: String,
    pub plan_name: String,
    pub base_fee_brl: f64,
    pub fx_usd_brl: Option<f64>,
    pub total_brl: f64,
    pub usage: TenantUsage,
    pub line_items: Vec<DraftLine>,
}

/// A computed line (mirrors `invoice_line_items` without ids).
#[derive(Debug, Clone, serde::Serialize)]
pub struct DraftLine {
    pub driver: String,
    pub qty: f64,
    pub unit: String,
    pub included: f64,
    pub billable_qty: f64,
    pub rate_brl: f64,
    pub amount_brl: f64,
}

/// A persisted invoice header.
#[derive(Debug, Clone, FromRow, serde::Serialize)]
pub struct Invoice {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub period: String,
    pub plan_id: String,
    pub plan_name: String,
    pub base_fee_brl: f64,
    pub fx_usd_brl: Option<f64>,
    pub total_brl: f64,
    pub status: String,
    pub created_at: String,
}

/// A persisted line item.
#[derive(Debug, Clone, FromRow, serde::Serialize)]
pub struct InvoiceLineItem {
    pub driver: String,
    pub qty: f64,
    pub unit: String,
    pub included: f64,
    pub billable_qty: f64,
    pub rate_brl: f64,
    pub amount_brl: f64,
}

/// Aggregate a tenant's metered usage for `period` ('YYYY-MM'), matched by RFC3339 prefix.
pub fn usage_for_period(db: &Db, tenant_id: Uuid, period: &str) -> TenantUsage {
    let like = format!("{period}%");
    let pool = db.pool();
    db.block(async move {
        let (gpu_ms, gpu_cost): (i64, f64) = sqlx::query_as(
            "SELECT COALESCE(SUM(delay_ms + exec_ms), 0)::bigint,
                    COALESCE(SUM(cost), 0)::double precision
             FROM ai_usage WHERE tenant_id = $1 AND created_at LIKE $2",
        )
        .bind(tenant_id)
        .bind(&like)
        .fetch_one(&pool)
        .await
        .unwrap_or((0, 0.0));

        let bytes: i64 = sqlx::query_scalar(
            "SELECT COALESCE(SUM(size_bytes), 0)::bigint
             FROM recordings WHERE tenant_id = $1 AND started_at LIKE $2",
        )
        .bind(tenant_id)
        .bind(&like)
        .fetch_one(&pool)
        .await
        .unwrap_or(0);

        let notifications: i64 = sqlx::query_scalar(
            "SELECT COUNT(*)::bigint FROM notification_queue
             WHERE tenant_id = $1 AND status = 'sent' AND created_at LIKE $2",
        )
        .bind(tenant_id)
        .bind(&like)
        .fetch_one(&pool)
        .await
        .unwrap_or(0);

        TenantUsage {
            gpu_sec: gpu_ms as f64 / 1000.0,
            gpu_cost_usd: gpu_cost,
            storage_gb: bytes as f64 / 1_000_000_000.0,
            notifications,
        }
    })
}

/// Compute (without persisting) the invoice for a tenant's `period`, from its current plan
/// + the period's usage + the latest FX rate. `None` if the tenant has no subscription.
pub fn compute(db: &Db, tenant_id: Uuid, period: &str) -> Option<InvoiceDraft> {
    let sub = subscriptions::current(db, tenant_id)?;
    let plan = plans::get(db, &sub.plan_id)?;
    let usage = usage_for_period(db, tenant_id, period);
    let fx_usd_brl = fx::latest(db, "USD", "BRL").map(|f| f.rate);

    // The plan base fee, then one metered line per driver. billable = max(0, used -
    // included); amount = billable * overage rate (BRL). v1 rates are 0 → amount 0.
    let line = |driver: &str, qty: f64, unit: &str, included: f64, rate: f64| DraftLine {
        driver: driver.to_string(),
        qty,
        unit: unit.to_string(),
        included,
        billable_qty: (qty - included).max(0.0),
        rate_brl: rate,
        amount_brl: (qty - included).max(0.0) * rate,
    };

    let line_items = vec![
        DraftLine {
            driver: "base".to_string(),
            qty: 1.0,
            unit: "month".to_string(),
            included: 0.0,
            billable_qty: 1.0,
            rate_brl: plan.base_fee_brl,
            amount_brl: plan.base_fee_brl,
        },
        line("gpu", usage.gpu_sec, "gpu_sec", plan.included_gpu_sec, plan.overage_gpu_per_sec),
        line("storage", usage.storage_gb, "gb", plan.included_storage_gb, plan.overage_storage_per_gb),
        line(
            "notifications",
            usage.notifications as f64,
            "count",
            plan.included_notifications as f64,
            plan.overage_notification_each,
        ),
    ];

    let total_brl = line_items.iter().map(|l| l.amount_brl).sum();

    Some(InvoiceDraft {
        tenant_id,
        period: period.to_string(),
        plan_id: plan.id,
        plan_name: plan.name,
        base_fee_brl: plan.base_fee_brl,
        fx_usd_brl,
        total_brl,
        usage,
        line_items,
    })
}

/// Close a period into an **immutable** `final` invoice. Idempotent: if a `final` invoice
/// already exists for (tenant, period) it is returned unchanged; a `draft` is replaced.
pub fn close_period(db: &Db, tenant_id: Uuid, period: &str) -> Result<Invoice, String> {
    if let Some(existing) = get(db, tenant_id, period) {
        if existing.status == "final" {
            return Ok(existing);
        }
    }
    let draft = compute(db, tenant_id, period).ok_or("tenant has no subscription to bill")?;
    let invoice_id = Uuid::new_v4();
    let now = now_rfc3339();
    let period_owned = period.to_string();
    let pool = db.pool();
    let res: Result<_, sqlx::Error> = db.block(async move {
        let mut tx = pool.begin().await?;
        // Replace any prior draft for this (tenant, period). Line items cascade on delete.
        sqlx::query("DELETE FROM invoices WHERE tenant_id = $1 AND period = $2")
            .bind(tenant_id)
            .bind(&period_owned)
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            "INSERT INTO invoices
                (id, tenant_id, period, plan_id, plan_name, base_fee_brl, fx_usd_brl,
                 total_brl, status, created_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, 'final', $9)",
        )
        .bind(invoice_id)
        .bind(tenant_id)
        .bind(&period_owned)
        .bind(&draft.plan_id)
        .bind(&draft.plan_name)
        .bind(draft.base_fee_brl)
        .bind(draft.fx_usd_brl)
        .bind(draft.total_brl)
        .bind(&now)
        .execute(&mut *tx)
        .await?;
        for l in &draft.line_items {
            sqlx::query(
                "INSERT INTO invoice_line_items
                    (id, invoice_id, driver, qty, unit, included, billable_qty, rate_brl, amount_brl)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
            )
            .bind(Uuid::new_v4())
            .bind(invoice_id)
            .bind(&l.driver)
            .bind(l.qty)
            .bind(&l.unit)
            .bind(l.included)
            .bind(l.billable_qty)
            .bind(l.rate_brl)
            .bind(l.amount_brl)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok::<_, sqlx::Error>(())
    });
    res.map_err(|e| format!("close_period: {e}"))?;
    get(db, tenant_id, period).ok_or_else(|| "close_period: invoice vanished after write".to_string())
}

/// One tenant's invoice for a period, if it exists.
pub fn get(db: &Db, tenant_id: Uuid, period: &str) -> Option<Invoice> {
    let pool = db.pool();
    let period = period.to_string();
    db.block(async move {
        sqlx::query_as::<_, Invoice>(
            "SELECT * FROM invoices WHERE tenant_id = $1 AND period = $2",
        )
        .bind(tenant_id)
        .bind(&period)
        .fetch_optional(&pool)
        .await
        .ok()
        .flatten()
    })
}

/// All of a tenant's invoices, newest period first.
pub fn list_for_tenant(db: &Db, tenant_id: Uuid) -> Vec<Invoice> {
    let pool = db.pool();
    db.block(async move {
        sqlx::query_as::<_, Invoice>(
            "SELECT * FROM invoices WHERE tenant_id = $1 ORDER BY period DESC",
        )
        .bind(tenant_id)
        .fetch_all(&pool)
        .await
        .unwrap_or_default()
    })
}

/// Every tenant's invoice for a period (master cross-tenant view).
pub fn list_for_period(db: &Db, period: &str) -> Vec<Invoice> {
    let pool = db.pool();
    let period = period.to_string();
    db.block(async move {
        sqlx::query_as::<_, Invoice>(
            "SELECT * FROM invoices WHERE period = $1 ORDER BY total_brl DESC",
        )
        .bind(&period)
        .fetch_all(&pool)
        .await
        .unwrap_or_default()
    })
}

/// The line items for an invoice.
pub fn line_items(db: &Db, invoice_id: Uuid) -> Vec<InvoiceLineItem> {
    let pool = db.pool();
    db.block(async move {
        sqlx::query_as::<_, InvoiceLineItem>(
            "SELECT driver, qty, unit, included, billable_qty, rate_brl, amount_brl
             FROM invoice_line_items WHERE invoice_id = $1 ORDER BY id",
        )
        .bind(invoice_id)
        .fetch_all(&pool)
        .await
        .unwrap_or_default()
    })
}
