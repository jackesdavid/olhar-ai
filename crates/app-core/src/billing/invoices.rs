//! Customer invoices: aggregate a period's usage, apply the tenant's plan, and (on close)
//! persist an immutable `final` invoice with its line items. v1 tiers are flat, so the
//! total is the base fee; metered lines are shown with amount 0 for transparency.
//!
//! Periods are `'YYYY-MM'` and matched by RFC3339 **prefix** (`created_at LIKE '2026-07%'`),
//! which is offset-agnostic — no fragile timestamp arithmetic.

use chrono::Datelike;
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
    /// **GB-month**: the average bytes we stored across the period's sampled days, not the
    /// bytes ingested. Storage is a stock — keeping 30 GB for 60 days costs 20x keeping it
    /// for 3, and a sum of uploads cannot tell those apart.
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
    pub currency: String,
    pub base_fee: f64,
    pub fx_usd_brl: Option<f64>,
    pub total: f64,
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
    pub rate: f64,
    pub amount: f64,
}

/// A persisted invoice header.
#[derive(Debug, Clone, FromRow, serde::Serialize)]
pub struct Invoice {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub period: String,
    pub plan_id: String,
    pub plan_name: String,
    pub currency: String,
    pub base_fee: f64,
    pub fx_usd_brl: Option<f64>,
    pub total: f64,
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
    pub rate: f64,
    pub amount: f64,
}

/// Aggregate a tenant's metered usage for `period` ('YYYY-MM'), matched by RFC3339 prefix.
pub fn usage_for_period(db: &Db, tenant_id: Uuid, period: &str) -> TenantUsage {
    let like = format!("{period}%");
    let period_key = period.to_string();
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

        // GB-month from the nightly samples (pg_cron, migration 0047). The old query summed
        // `recordings.size_bytes` for the period — bytes sitting on the *edge's* own disk,
        // which we never stored and must not bill.
        let avg_bytes: f64 = sqlx::query_scalar(
            "SELECT COALESCE(AVG(bytes), 0)::double precision
             FROM tenant_storage_daily
             WHERE tenant_id = $1 AND to_char(day, 'YYYY-MM') = $2",
        )
        .bind(tenant_id)
        .bind(&period_key)
        .fetch_one(&pool)
        .await
        .unwrap_or(0.0);

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
            storage_gb: avg_bytes / 1_000_000_000.0,
            notifications,
        }
    })
}

/// Compute (without persisting) the invoice for a tenant's `period`, from its current plan
/// + the period's usage + the latest FX rate. `None` if the tenant has no subscription.
pub fn compute(db: &Db, tenant_id: Uuid, period: &str) -> Option<InvoiceDraft> {
    // The subscription in force during `period` — NOT today's. Billing a past month with
    // the current plan would invoice a tenant for a month they hadn't subscribed.
    let sub = subscriptions::active_during(db, tenant_id, period)?;
    let currency = super::tenant_currency(db, tenant_id);
    // Quotas come from the tier, rates from the price row — money is per-currency, a GB is not.
    let plan = plans::priced(db, &sub.plan_id, &currency)?;
    let base_fee = plan.amount;
    let usage = usage_for_period(db, tenant_id, period);
    let fx_usd_brl = fx::latest(db, "USD", "BRL").map(|f| f.rate);

    // The plan base fee, then one metered line per driver. billable = max(0, used -
    // included); amount = billable * overage rate (invoice currency). v1 rates are 0 → 0.
    let line = |driver: &str, qty: f64, unit: &str, included: f64, rate: f64| DraftLine {
        driver: driver.to_string(),
        qty,
        unit: unit.to_string(),
        included,
        billable_qty: (qty - included).max(0.0),
        rate,
        amount: (qty - included).max(0.0) * rate,
    };

    let line_items = vec![
        DraftLine {
            driver: "base".to_string(),
            qty: 1.0,
            unit: "month".to_string(),
            included: 0.0,
            billable_qty: 1.0,
            rate: base_fee,
            amount: base_fee,
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

    let total = line_items.iter().map(|l| l.amount).sum();

    Some(InvoiceDraft {
        tenant_id,
        period: period.to_string(),
        plan_id: plan.id,
        plan_name: plan.name,
        currency,
        base_fee,
        fx_usd_brl,
        total,
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
    let draft = compute(db, tenant_id, period).ok_or("tenant had no subscription during that period")?;
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
                (id, tenant_id, period, plan_id, plan_name, currency, base_fee, fx_usd_brl,
                 total, status, created_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, 'final', $10)",
        )
        .bind(invoice_id)
        .bind(tenant_id)
        .bind(&period_owned)
        .bind(&draft.plan_id)
        .bind(&draft.plan_name)
        .bind(&draft.currency)
        .bind(draft.base_fee)
        .bind(draft.fx_usd_brl)
        .bind(draft.total)
        .bind(&now)
        .execute(&mut *tx)
        .await?;
        for l in &draft.line_items {
            sqlx::query(
                "INSERT INTO invoice_line_items
                    (id, invoice_id, driver, qty, unit, included, billable_qty, rate, amount)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
            )
            .bind(Uuid::new_v4())
            .bind(invoice_id)
            .bind(&l.driver)
            .bind(l.qty)
            .bind(&l.unit)
            .bind(l.included)
            .bind(l.billable_qty)
            .bind(l.rate)
            .bind(l.amount)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok::<_, sqlx::Error>(())
    });
    res.map_err(|e| format!("close_period: {e}"))?;
    get(db, tenant_id, period).ok_or_else(|| "close_period: invoice vanished after write".to_string())
}

/// Close `period` for every tenant that has a plan. Returns `(closed, skipped)`.
///
/// Idempotent by construction — [`close_period`] returns an existing `final` invoice
/// untouched — so it is safe to run from several control replicas.
pub fn close_period_all(db: &Db, period: &str) -> (usize, usize) {
    let (mut closed, mut skipped) = (0, 0);
    for row in subscriptions::overview(db) {
        // Not a customer during `period` (or ever) → nothing to bill.
        if row.plan_id.is_none() || subscriptions::active_during(db, row.tenant_id, period).is_none() {
            skipped += 1;
            continue;
        }
        match close_period(db, row.tenant_id, period) {
            Ok(_) => closed += 1,
            Err(e) => {
                skipped += 1;
                log::warn!("invoices: close {period} for {} failed: {e}", row.tenant_name);
            }
        }
    }
    (closed, skipped)
}

/// The calendar month before `now`, as `YYYY-MM` — the period a monthly job should close.
pub fn previous_period() -> String {
    let now = chrono::Local::now().date_naive();
    let first = now.with_day(1).unwrap_or(now);
    let prev = first.pred_opt().unwrap_or(first);
    prev.format("%Y-%m").to_string()
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
            "SELECT * FROM invoices WHERE period = $1 ORDER BY total DESC",
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
            "SELECT driver, qty, unit, included, billable_qty, rate, amount
             FROM invoice_line_items WHERE invoice_id = $1 ORDER BY id",
        )
        .bind(invoice_id)
        .fetch_all(&pool)
        .await
        .unwrap_or_default()
    })
}
