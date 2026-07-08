//! Per-tenant billing / cost metering.
//!
//! Phase 1: **AI/GPU usage metering**. Every call to the RunPod AI sidecar
//! ([`crate::agent::ai_service`]) is recorded against its tenant, using the billed time
//! the RunPod envelope reports (`delayTime` + `executionTime`). RunPod bills the whole
//! Serverless endpoint by GPU-second (there is no per-request cost center), so we do the
//! per-tenant attribution ourselves — we know the tenant at call time. Rows are
//! append-only; a metering write must never fail the AI operation it measures.
//!
//! Phase 2: the **plan / subscription / FX** model — [`plans`] (the tier catalog),
//! [`subscriptions`] (per-tenant plan + Beta trial lifecycle), and [`fx`] (USD↔BRL rates
//! with a last-quote fallback). Customer invoices (Phase 3) build on these.

pub mod fx;
pub mod invoices;
pub mod payments;
pub mod plans;
pub mod subscriptions;

use uuid::Uuid;

use crate::alerts::now_rfc3339;
use crate::db::Db;

/// The currency a tenant is billed in (`BRL` for Brazil, `USD` for the American market).
/// Defaults to `BRL` when the tenant row is missing or unset.
pub fn tenant_currency(db: &Db, tenant_id: Uuid) -> String {
    let pool = db.pool();
    db.block(async move {
        sqlx::query_scalar::<_, String>("SELECT currency FROM tenants WHERE id = $1")
            .bind(tenant_id)
            .fetch_optional(&pool)
            .await
            .ok()
            .flatten()
    })
    .unwrap_or_else(|| "BRL".to_string())
}

/// Configured price per GPU-second for the AI endpoint (currency-agnostic — the caller's
/// rate unit). Read from `AI_RATE_PER_SEC`; defaults to `0.0` so metering still records
/// the billed milliseconds even before a rate is set (cost then stays `0`). The rate
/// depends on the GPU behind the endpoint (e.g. RTX 2060 vs H100).
pub fn ai_rate_per_sec() -> f64 {
    std::env::var("AI_RATE_PER_SEC")
        .ok()
        .and_then(|s| s.trim().parse::<f64>().ok())
        .filter(|r| r.is_finite() && *r >= 0.0)
        .unwrap_or(0.0)
}

/// Record one AI-sidecar call against `tenant_id`. Append-only and **fire-and-forget**:
/// billing must never break the AI path, so a failed write is logged, not returned.
///
/// `delay_ms` + `exec_ms` are the RunPod-billed time from the response envelope;
/// `cost = (delay_ms + exec_ms) / 1000 × rate`. Local (`--rp_serve_api`) calls report no
/// timing, so both are `0` and the row is a zero-cost audit record.
pub fn record_ai_usage(
    db: &Db,
    tenant_id: Uuid,
    op: &str,
    delay_ms: u32,
    exec_ms: u32,
    success: bool,
) {
    let id = Uuid::new_v4();
    let now = now_rfc3339();
    let rate = ai_rate_per_sec();
    let cost = (delay_ms as f64 + exec_ms as f64) / 1000.0 * rate;
    let pool = db.pool();
    let op_owned = op.to_string();
    let res: Result<_, sqlx::Error> = db.block(async move {
        sqlx::query(
            "INSERT INTO ai_usage
                (id, tenant_id, op, delay_ms, exec_ms, success, rate_per_sec, cost, created_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
        )
        .bind(id)
        .bind(tenant_id)
        .bind(&op_owned)
        .bind(delay_ms as i32)
        .bind(exec_ms as i32)
        .bind(success)
        .bind(rate)
        .bind(cost)
        .bind(&now)
        .execute(&pool)
        .await
    });
    if let Err(e) = res {
        log::error!("billing::record_ai_usage failed (tenant {tenant_id}, op '{op}'): {e}");
    }
}
