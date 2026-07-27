//! In-memory log capture for the edge. A bounded ring buffer fed by a `tracing` layer (which also
//! tees to stderr) and by the go2rtc child reader, so the box's recent logs can be pushed up to the
//! cloud — the edge is no longer a black box. The buffer is process-global so the layer, the go2rtc
//! readers, and the push loop all share one source.
//!
//! **Card #146 — structured fields.** Each line carries a `fields` map: telemetry sites emit typed
//! `tracing` events (`tracing::info!(camera_ip=%ip, fps, …, "capture")`) and the layer captures every
//! field as a searchable JSON key. The relay ships `fields` up; control stamps the box identity
//! (`edge_id`/`edge_name`/`tenant`) and forwards the structured record to Datadog. Existing `log::*`
//! sites keep working via the `tracing-log` bridge (their args flatten into `message`, no `fields`).
use std::collections::VecDeque;
use std::sync::{Arc, Mutex, OnceLock};

use serde_json::{Map, Value};

/// One captured log line. `seq` is a monotonic cursor used by the push loop; it is not
/// sent on the wire (the cloud assigns its own ids).
#[derive(Clone, serde::Serialize)]
pub struct LogLine {
    #[serde(skip_serializing)]
    pub seq: u64,
    pub ts: String,
    pub level: String,
    pub target: String,
    pub message: String,
    /// Structured fields of the event (card #146). Empty for plaintext (`log::*`) lines and the
    /// go2rtc child reader; skipped on the wire when empty so old lines stay compact.
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub fields: Map<String, Value>,
}

struct Ring {
    buf: VecDeque<LogLine>,
    next_seq: u64,
}

/// A shared, bounded ring of recent log lines.
#[derive(Clone)]
pub struct LogBuf(Arc<Mutex<Ring>>);

const CAP: usize = 5000;
static RING: OnceLock<LogBuf> = OnceLock::new();

impl LogBuf {
    /// The process-global buffer (created on first use).
    pub fn global() -> LogBuf {
        RING.get_or_init(|| {
            LogBuf(Arc::new(Mutex::new(Ring {
                buf: VecDeque::new(),
                next_seq: 1,
            })))
        })
        .clone()
    }

    /// Append a plaintext line with no structured fields (the go2rtc child reader).
    pub fn push(&self, level: &str, target: &str, message: String) {
        self.push_fields(level, target, message, Map::new());
    }

    /// Append a line with structured fields (oldest dropped past [`CAP`]).
    pub fn push_fields(&self, level: &str, target: &str, message: String, fields: Map<String, Value>) {
        let mut r = self.0.lock().unwrap();
        let seq = r.next_seq;
        r.next_seq += 1;
        r.buf.push_back(LogLine {
            seq,
            ts: app_core::timeutil::now_rfc3339(),
            level: level.to_string(),
            target: target.to_string(),
            message,
            fields,
        });
        while r.buf.len() > CAP {
            r.buf.pop_front();
        }
    }

    /// Lines newer than `after`, plus the new cursor (the latest seq held).
    pub fn since(&self, after: u64) -> (Vec<LogLine>, u64) {
        let r = self.0.lock().unwrap();
        let lines: Vec<LogLine> = r.buf.iter().filter(|l| l.seq > after).cloned().collect();
        let cursor = r.buf.back().map(|l| l.seq).unwrap_or(after);
        (lines, cursor)
    }
}

/// Flattens `tracing` event/span fields into a JSON map (each field a searchable key).
#[derive(Default)]
struct JsonVisitor(Map<String, Value>);
impl tracing::field::Visit for JsonVisitor {
    fn record_str(&mut self, f: &tracing::field::Field, v: &str) {
        self.0.insert(f.name().into(), Value::from(v));
    }
    fn record_i64(&mut self, f: &tracing::field::Field, v: i64) {
        self.0.insert(f.name().into(), Value::from(v));
    }
    fn record_u64(&mut self, f: &tracing::field::Field, v: u64) {
        self.0.insert(f.name().into(), Value::from(v));
    }
    fn record_f64(&mut self, f: &tracing::field::Field, v: f64) {
        self.0.insert(f.name().into(), Value::from(v));
    }
    fn record_bool(&mut self, f: &tracing::field::Field, v: bool) {
        self.0.insert(f.name().into(), Value::from(v));
    }
    fn record_debug(&mut self, f: &tracing::field::Field, v: &dyn std::fmt::Debug) {
        self.0.insert(f.name().into(), Value::from(format!("{v:?}")));
    }
}

/// A span's own fields, stashed so events inside inherit them.
struct SpanFields(Map<String, Value>);

/// A `tracing` layer that tees each event to stderr (like env_logger did) and into the global ring
/// with its structured fields. The whole edge is one box, so identity is stamped centrally by control
/// at relay time (from the box token) — this layer just carries the event's own telemetry fields.
struct RingLayer {
    buf: LogBuf,
}

impl<S> tracing_subscriber::Layer<S> for RingLayer
where
    S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
{
    fn on_new_span(&self, attrs: &tracing::span::Attributes<'_>, id: &tracing::span::Id, ctx: tracing_subscriber::layer::Context<'_, S>) {
        let mut v = JsonVisitor::default();
        attrs.record(&mut v);
        if let Some(span) = ctx.span(id) {
            span.extensions_mut().insert(SpanFields(v.0));
        }
    }

    fn on_event(&self, event: &tracing::Event<'_>, ctx: tracing_subscriber::layer::Context<'_, S>) {
        // Merge ambient span fields, then the event's own fields.
        let mut fields = Map::new();
        if let Some(scope) = ctx.event_scope(event) {
            for span in scope.from_root() {
                if let Some(sf) = span.extensions().get::<SpanFields>() {
                    for (k, val) in &sf.0 {
                        fields.insert(k.clone(), val.clone());
                    }
                }
            }
        }
        let mut ev = JsonVisitor::default();
        event.record(&mut ev);
        let message = ev
            .0
            .remove("message")
            .and_then(|m| m.as_str().map(str::to_string))
            .unwrap_or_default();
        for (k, val) in ev.0 {
            fields.insert(k, val);
        }

        let meta = event.metadata();
        let level = meta.level().as_str().to_lowercase();
        let target = meta.target();
        let ts = app_core::timeutil::now_rfc3339();
        if fields.is_empty() {
            eprintln!("[{ts} {level} {target}] {message}");
        } else {
            eprintln!("[{ts} {level} {target}] {message} {}", Value::Object(fields.clone()));
        }
        self.buf.push_fields(&level, target, message, fields);
    }
}

/// Take the level word out of `RUST_LOG` (handles `info`, `module=debug,info`, …), else info.
fn env_filter() -> tracing_subscriber::EnvFilter {
    tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"))
}

/// Install the capturing subscriber. Returns the shared buffer for the push loop. Call once,
/// before anything logs. Existing `log::*` calls are bridged into tracing so they land in the ring
/// too (as plaintext `message`, no `fields`).
pub fn init() -> LogBuf {
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;

    let buf = LogBuf::global();
    let _ = tracing_subscriber::registry()
        .with(env_filter())
        .with(RingLayer { buf: buf.clone() })
        .try_init();
    let _ = tracing_log::LogTracer::init();
    buf
}
