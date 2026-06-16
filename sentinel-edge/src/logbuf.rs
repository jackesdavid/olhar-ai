//! In-memory log capture for the edge. A bounded ring buffer fed by a custom
//! [`log::Log`] (which also tees to stderr) and by the go2rtc child reader, so the box's
//! recent logs can be pushed up to the cloud for in-UI observability — the edge is no
//! longer a black box. The buffer is process-global so the logger, the go2rtc readers,
//! and the push loop all share one source.
use std::collections::VecDeque;
use std::sync::{Arc, Mutex, OnceLock};

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
        RING.get_or_init(|| LogBuf(Arc::new(Mutex::new(Ring { buf: VecDeque::new(), next_seq: 1 })))).clone()
    }

    /// Append a line (oldest dropped past [`CAP`]).
    pub fn push(&self, level: &str, target: &str, message: String) {
        let mut r = self.0.lock().unwrap();
        let seq = r.next_seq;
        r.next_seq += 1;
        r.buf.push_back(LogLine {
            seq,
            ts: app_core::alerts::now_rfc3339(),
            level: level.to_string(),
            target: target.to_string(),
            message,
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

/// A custom logger that tees each record to stderr (like env_logger did) and into the
/// global ring. Level threshold comes from `RUST_LOG` (a level word anywhere), else info.
struct RingLogger {
    level: log::LevelFilter,
    buf: LogBuf,
}

impl log::Log for RingLogger {
    fn enabled(&self, m: &log::Metadata) -> bool {
        m.level() <= self.level
    }
    fn log(&self, record: &log::Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let level = record.level();
        let target = record.target().to_string();
        let message = format!("{}", record.args());
        let ts = app_core::alerts::now_rfc3339();
        eprintln!("[{ts} {level} {target}] {message}");
        self.buf.push(&level.to_string().to_lowercase(), &target, message);
    }
    fn flush(&self) {}
}

/// Take the level word out of `RUST_LOG` (handles `info`, `module=debug,info`, …), else info.
fn level_from_env() -> log::LevelFilter {
    let v = std::env::var("RUST_LOG").unwrap_or_default().to_ascii_lowercase();
    if v.contains("trace") {
        log::LevelFilter::Trace
    } else if v.contains("debug") {
        log::LevelFilter::Debug
    } else if v.contains("warn") {
        log::LevelFilter::Warn
    } else if v.contains("error") {
        log::LevelFilter::Error
    } else {
        log::LevelFilter::Info
    }
}

/// Install the capturing logger. Returns the shared buffer for the push loop. Call once,
/// before anything logs.
pub fn init() -> LogBuf {
    let buf = LogBuf::global();
    let level = level_from_env();
    log::set_max_level(level);
    let _ = log::set_boxed_logger(Box::new(RingLogger { level, buf: buf.clone() }));
    buf
}
