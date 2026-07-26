//! Ship logs to Datadog's Logs Intake API (card #145) — the sink that replaces Postgres for the
//! high-volume, low-value log/trace streams (`platform_logs`, the edge `box_logs` relay), so they
//! stop competing with the product event stream + client data for DB write/vacuum load.
//!
//! **Env-gated.** `DD_API_KEY` unset → disabled, and every caller falls back to its Postgres sink,
//! so dev/local and any deploy without a key keeps working unchanged. `DD_SITE` picks the region
//! (default `datadoghq.com`).
//!
//! Uses **ureq** (already an app-core dep; blocking) so there is no new dependency and no bloat on
//! the edge build — which never ships to Datadog anyway (the edge relays its lines to control, and
//! control does the shipping). Async callers wrap [`ship_blocking`] in `spawn_blocking`.

use serde_json::Value;
use std::sync::OnceLock;

struct Config {
    api_key: String,
    url: String,
}

fn config() -> Option<&'static Config> {
    static C: OnceLock<Option<Config>> = OnceLock::new();
    C.get_or_init(|| {
        let api_key = std::env::var("DD_API_KEY").ok().filter(|s| !s.trim().is_empty())?;
        let site = std::env::var("DD_SITE")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| "datadoghq.com".to_string());
        Some(Config { api_key, url: format!("https://http-intake.logs.{site}/api/v2/logs") })
    })
    .as_ref()
}

/// Whether Datadog log shipping is configured (`DD_API_KEY` set). When `false`, callers keep their
/// Postgres sink — this is the switch that moves logs off the database.
pub fn configured() -> bool {
    config().is_some()
}

/// Ship a batch of log records to Datadog (blocking, best-effort). Each record is a Datadog log
/// object (`{ddsource, service, ddtags, level, message, …}`). A failed POST drops the batch — a lost
/// debug line is acceptable; log shipping must never block a request or crash the caller.
pub fn ship_blocking(records: &[Value]) {
    let Some(cfg) = config() else { return };
    if records.is_empty() {
        return;
    }
    let resp = ureq::post(&cfg.url)
        .set("DD-API-KEY", &cfg.api_key)
        .set("Content-Type", "application/json")
        .timeout(std::time::Duration::from_secs(10))
        .send_json(records);
    if let Err(e) = resp {
        log::debug!("datadog ship failed ({} records): {e}", records.len());
    }
}
