//! Ship the gateway's logs to Datadog (card #146). The gateway is standalone — no `app-core`, no DB
//! by design — so this is a **minimal, self-contained copy** of the tracing→Datadog layer that lives
//! in `app-core::datadog` for control/flow/edge. It uses `reqwest` (already a dep) so no new HTTP
//! stack, and tags every record `service:gateway`.
//!
//! **Env-gated.** `DD_API_KEY` unset → the layer is a no-op (stderr only), so dev keeps working.
//! `DD_SITE` picks the region (default `datadoghq.com`).
use serde_json::{Map, Value};
use std::sync::OnceLock;
use tracing::field::{Field, Visit};
use tracing_subscriber::layer::{Context, Layer};
use tracing_subscriber::registry::LookupSpan;

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

/// The record queue → drained by an async task that batches + POSTs to Datadog. `try_send` from the
/// (sync) tracing layer never blocks; drops on backpressure — a lost debug line is acceptable.
fn queue() -> Option<&'static tokio::sync::mpsc::Sender<Value>> {
    static Q: OnceLock<Option<tokio::sync::mpsc::Sender<Value>>> = OnceLock::new();
    Q.get_or_init(|| {
        let cfg = config()?;
        let (tx, rx) = tokio::sync::mpsc::channel::<Value>(20_000);
        tokio::spawn(drain(rx, &cfg.url, &cfg.api_key));
        Some(tx)
    })
    .as_ref()
}

async fn drain(mut rx: tokio::sync::mpsc::Receiver<Value>, url: &'static str, api_key: &'static str) {
    let client = reqwest::Client::new();
    let mut batch: Vec<Value> = Vec::with_capacity(500);
    loop {
        // Wait for the first record, then coalesce whatever else is queued (up to 500) before POSTing.
        let Some(first) = rx.recv().await else { return };
        batch.push(first);
        while batch.len() < 500 {
            match rx.try_recv() {
                Ok(r) => batch.push(r),
                Err(_) => break,
            }
        }
        let res = client
            .post(url)
            .header("DD-API-KEY", api_key)
            .json(&batch)
            .send()
            .await;
        if let Err(e) = res {
            eprintln!("datadog ship failed ({} records): {e}", batch.len());
        }
        batch.clear();
    }
}

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
        self.0.insert(f.name().into(), Value::from(format!("{v:?}")));
    }
}

/// A span's own fields, stashed so events inside inherit them.
struct SpanFields(Map<String, Value>);

/// Ships every tracing event to Datadog as a structured record, tagged `service:gateway`.
struct DatadogLayer;

impl<S> Layer<S> for DatadogLayer
where
    S: tracing::Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_new_span(&self, attrs: &tracing::span::Attributes<'_>, id: &tracing::span::Id, ctx: Context<'_, S>) {
        let mut v = JsonVisitor::default();
        attrs.record(&mut v);
        if let Some(span) = ctx.span(id) {
            span.extensions_mut().insert(SpanFields(v.0));
        }
    }

    fn on_event(&self, event: &tracing::Event<'_>, ctx: Context<'_, S>) {
        let Some(tx) = queue() else { return };
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
        let message = ev.0.remove("message").and_then(|m| m.as_str().map(str::to_string)).unwrap_or_default();
        for (k, val) in ev.0 {
            merged.insert(k, val);
        }

        let meta = event.metadata();
        let tags = "service:gateway".to_string();
        let mut record = Map::new();
        record.insert("ddsource".into(), Value::from("sentinel"));
        record.insert("service".into(), Value::from("gateway"));
        record.insert("status".into(), Value::from(meta.level().as_str().to_lowercase()));
        record.insert("logger.name".into(), Value::from(meta.target()));
        record.insert("ddtags".into(), Value::from(tags));
        record.insert("message".into(), Value::from(message));
        for (k, val) in merged {
            record.entry(k).or_insert(val);
        }
        let _ = tx.try_send(Value::Object(record));
    }
}

/// Install the tracing subscriber: stderr + the Datadog layer + the `log::*` bridge. Call once at
/// boot from inside the tokio runtime (the drain task needs it). Replaces `env_logger::init()`.
pub fn init_tracing() {
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;

    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    let _ = tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer().with_target(true))
        .with(DatadogLayer)
        .try_init();
    let _ = tracing_log::LogTracer::init();
}
