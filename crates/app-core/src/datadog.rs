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
        let api_key = std::env::var("DD_API_KEY")
            .ok()
            .filter(|s| !s.trim().is_empty())?;
        let site = std::env::var("DD_SITE")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| "datadoghq.com".to_string());
        Some(Config {
            api_key,
            url: format!("https://http-intake.logs.{site}/api/v2/logs"),
        })
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
        // Avoid `log::` here — this runs on the shipper thread and could recurse via the tracing
        // bridge. `eprintln!` goes to stderr/docker only.
        eprintln!("datadog ship failed ({} records): {e}", records.len());
    }
}

// ---- batched queue (card #146) — one background thread coalesces events into POST batches -------

/// Enqueue one structured log record for Datadog (non-blocking; drops on backpressure). The tracing
/// layer calls this per event.
pub fn enqueue(record: Value) {
    if let Some(tx) = queue() {
        let _ = tx.try_send(record);
    }
}

fn queue() -> Option<&'static std::sync::mpsc::SyncSender<Value>> {
    static Q: OnceLock<Option<std::sync::mpsc::SyncSender<Value>>> = OnceLock::new();
    Q.get_or_init(|| {
        if !configured() {
            return None;
        }
        let (tx, rx) = std::sync::mpsc::sync_channel::<Value>(20_000);
        let _ = std::thread::Builder::new()
            .name("datadog-logs".into())
            .spawn(move || drain(rx));
        Some(tx)
    })
    .as_ref()
}

fn drain(rx: std::sync::mpsc::Receiver<Value>) {
    let mut batch: Vec<Value> = Vec::with_capacity(500);
    loop {
        match rx.recv_timeout(std::time::Duration::from_secs(2)) {
            Ok(first) => {
                batch.push(first);
                while batch.len() < 500 {
                    match rx.try_recv() {
                        Ok(r) => batch.push(r),
                        Err(_) => break,
                    }
                }
                ship_blocking(&batch);
                batch.clear();
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
        }
    }
}

// ---- tracing layer (card #146) — every event → a structured Datadog record ----------------------

mod tracing_layer {
    use super::{configured, enqueue};
    use serde_json::{Map, Value};
    use tracing::field::{Field, Visit};
    use tracing_subscriber::layer::{Context, Layer};
    use tracing_subscriber::registry::LookupSpan;

    /// Flattens tracing fields into a JSON map (each field a searchable key).
    #[derive(Default)]
    struct JsonVisitor(Map<String, Value>);
    impl Visit for JsonVisitor {
        fn record_str(&mut self, f: &Field, v: &str) {
            self.0.insert(f.name().into(), Value::from(v));
        }
        fn record_i64(&mut self, f: &Field, v: i64) {
            self.0.insert(f.name().into(), Value::from(v));
        }
        fn record_u64(&mut self, f: &Field, v: u64) {
            self.0.insert(f.name().into(), Value::from(v));
        }
        fn record_f64(&mut self, f: &Field, v: f64) {
            self.0.insert(f.name().into(), Value::from(v));
        }
        fn record_bool(&mut self, f: &Field, v: bool) {
            self.0.insert(f.name().into(), Value::from(v));
        }
        fn record_debug(&mut self, f: &Field, v: &dyn std::fmt::Debug) {
            self.0
                .insert(f.name().into(), Value::from(format!("{v:?}")));
        }
    }

    /// A span's own fields, stashed so events inside inherit them (the tenant/edge context).
    struct SpanFields(Map<String, Value>);

    /// Ships every tracing event to Datadog as a structured record. `service` tags the source
    /// (control | flow | edge | gateway); span-scope fields (tenant, edge_id, edge_name) merge in.
    pub struct DatadogLayer {
        pub service: &'static str,
    }

    impl<S> Layer<S> for DatadogLayer
    where
        S: tracing::Subscriber + for<'a> LookupSpan<'a>,
    {
        fn on_new_span(
            &self,
            attrs: &tracing::span::Attributes<'_>,
            id: &tracing::span::Id,
            ctx: Context<'_, S>,
        ) {
            let mut v = JsonVisitor::default();
            attrs.record(&mut v);
            if let Some(span) = ctx.span(id) {
                span.extensions_mut().insert(SpanFields(v.0));
            }
        }

        fn on_event(&self, event: &tracing::Event<'_>, ctx: Context<'_, S>) {
            if !configured() {
                return;
            }
            // Merge parent-span context (tenant/edge), then the event's own fields.
            let mut merged = Map::new();
            if let Some(scope) = ctx.event_scope(event) {
                for span in scope.from_root() {
                    if let Some(sf) = span.extensions().get::<SpanFields>() {
                        for (k, val) in &sf.0 {
                            merged.insert(k.clone(), val.clone());
                        }
                    }
                }
            }
            let mut ev = JsonVisitor::default();
            event.record(&mut ev);
            let message =
                ev.0.remove("message")
                    .and_then(|m| m.as_str().map(str::to_string))
                    .unwrap_or_default();
            for (k, val) in ev.0 {
                merged.insert(k, val);
            }

            let meta = event.metadata();
            let mut tags = format!("service:{}", self.service);
            if let Some(t) = merged
                .get("tenant")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
            {
                tags.push_str(&format!(",tenant:{t}"));
            }
            if let Some(e) = merged
                .get("edge_id")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
            {
                tags.push_str(&format!(",edge_id:{e}"));
            }
            if let Some(n) = merged
                .get("edge_name")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
            {
                tags.push_str(&format!(",edge_name:{n}"));
            }

            let mut record = Map::new();
            record.insert("ddsource".into(), Value::from("sentinel"));
            record.insert("service".into(), Value::from(self.service));
            record.insert(
                "status".into(),
                Value::from(meta.level().as_str().to_lowercase()),
            ); // DD 'status' = level
            record.insert("logger.name".into(), Value::from(meta.target()));
            record.insert("ddtags".into(), Value::from(tags));
            record.insert("message".into(), Value::from(message));
            for (k, val) in merged {
                record.entry(k).or_insert(val); // don't clobber the reserved keys above
            }
            enqueue(Value::Object(record));
        }
    }
}

/// Install the tracing subscriber for a `service` (control | flow | edge | gateway): pretty stderr +
/// the Datadog layer + the `log::*` → tracing bridge, so EVERY log (info + errors, from `log!` or
/// `tracing!`) ships to Datadog with `service:` + span context. Call once at boot, replacing
/// `env_logger::init()`. When `DD_API_KEY` is unset the Datadog layer is a no-op (stderr only).
pub fn init_tracing(service: &'static str) {
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;

    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    let _ = tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer().with_target(true))
        .with(tracing_layer::DatadogLayer { service })
        .try_init();
    // Route existing `log::*` records through tracing so they reach the Datadog layer too.
    let _ = tracing_log::LogTracer::init();
}
