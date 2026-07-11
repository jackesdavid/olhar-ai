//! Exchange rates (`fx_rates`): append-only log + a fetcher with last-quote fallback.
//! Our costs are USD; BRL is the billing currency. A daily refresh records one row; if
//! the fetch fails we skip the cycle and the last recorded rate stays authoritative.

use std::time::Duration;

use sqlx::FromRow;
use uuid::Uuid;

use crate::timeutil::now_rfc3339;
use crate::db::Db;

#[derive(Debug, Clone, FromRow, serde::Serialize)]
pub struct FxRate {
    pub base: String,
    pub quote: String,
    /// 1 `base` = `rate` `quote`.
    pub rate: f64,
    pub source: String,
    pub fetched_at: String,
}

/// The most recent rate for `base → quote` (the fallback used when a refresh fails).
pub fn latest(db: &Db, base: &str, quote: &str) -> Option<FxRate> {
    let pool = db.pool();
    let base = base.to_string();
    let quote = quote.to_string();
    db.block(async move {
        sqlx::query_as::<_, FxRate>(
            "SELECT base, quote, rate, source, fetched_at FROM fx_rates
             WHERE base = $1 AND quote = $2 ORDER BY fetched_at DESC LIMIT 1",
        )
        .bind(&base)
        .bind(&quote)
        .fetch_optional(&pool)
        .await
        .ok()
        .flatten()
    })
}

/// Append one rate to the log.
pub fn record(db: &Db, base: &str, quote: &str, rate: f64, source: &str) {
    let id = Uuid::new_v4();
    let now = now_rfc3339();
    let base = base.to_string();
    let quote = quote.to_string();
    let source = source.to_string();
    let pool = db.pool();
    let res: Result<_, sqlx::Error> = db.block(async move {
        sqlx::query(
            "INSERT INTO fx_rates (id, base, quote, rate, source, fetched_at)
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(id)
        .bind(&base)
        .bind(&quote)
        .bind(rate)
        .bind(&source)
        .bind(&now)
        .execute(&pool)
        .await
    });
    if let Err(e) = res {
        log::error!("fx::record failed: {e}");
    }
}

/// Fetch the live USD→BRL rate and record it. On any failure the last recorded rate stays
/// authoritative (we just skip this cycle) — so there is always a quote to fall back to.
pub fn refresh_usd_brl(db: &Db) {
    match fetch_usd_brl() {
        Ok(rate) => {
            record(db, "USD", "BRL", rate, SOURCE);
            log::info!("fx: USD→BRL = {rate}");
        }
        Err(e) => log::warn!("fx: USD→BRL refresh failed (keeping last quote): {e}"),
    }
}

const SOURCE: &str = "open.er-api.com";

/// Pull USD→BRL from a free, key-less provider.
fn fetch_usd_brl() -> Result<f64, String> {
    let body: serde_json::Value = ureq::get("https://open.er-api.com/v6/latest/USD")
        .timeout(Duration::from_secs(15))
        .call()
        .map_err(|e| format!("request failed: {e}"))?
        .into_json()
        .map_err(|e| format!("bad JSON: {e}"))?;
    body.get("rates")
        .and_then(|r| r.get("BRL"))
        .and_then(serde_json::Value::as_f64)
        .filter(|r| r.is_finite() && *r > 0.0)
        .ok_or_else(|| "no BRL rate in response".to_string())
}
