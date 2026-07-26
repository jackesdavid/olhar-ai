//! Unified platform audit log — one write path for every module.
//!
//! Two insertion paths:
//! - [`insert_log_sync`] / [`insert_logs_sync`]: blocking (Db::block), for use from
//!   std threads in sentinel-flow's engine — no tokio context required at call site.
//! - [`LogQueue`]: async, fire-and-forget mpsc → batched insert, for use from
//!   sentinel-control's async handlers.
//!
//! One read path: [`list_logs`] (async, paginated) consumed by the control-plane API.
//!
//! **Trace-ID rule**: modules that receive a trace_id from a parent action (e.g. a
//! workflow run triggering a sub-run or a presence event) MUST pass it through —
//! never generate a new trace_id when one is already in scope.
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

use crate::db::Db;

/// One platform log entry. `created_at` is set by the insert functions (now_rfc3339).
pub struct PlatformLog {
    pub tenant_id:   Uuid,
    pub module:      &'static str,   // "workflow" | "edge" | "presence" | "device"
    pub trace_id:    Option<Uuid>,   // links entries across modules for one root action
    pub entity_id:   Option<String>, // graph_id (as text), box_id, device_id, ...
    pub entity_type: Option<&'static str>, // "workflow_graph" | "edge_box" | "device" | ...
    pub level:       &'static str,   // "debug" | "info" | "warn" | "error"
    pub message:     String,
    pub payload:     Option<Value>,
}

/// A platform log as a Datadog log record (card #145) — the SQL filter columns become `ddtags` so
/// Datadog's facets replace the `WHERE module=/entity_id=/level=` queries the in-DB viewer used.
fn to_dd_record(l: &PlatformLog) -> Value {
    let mut tags = format!("tenant:{},module:{}", l.tenant_id, l.module);
    if let Some(et) = l.entity_type {
        tags.push_str(&format!(",entity_type:{et}"));
    }
    if let Some(eid) = &l.entity_id {
        tags.push_str(&format!(",entity_id:{eid}"));
    }
    serde_json::json!({
        "ddsource": "sentinel",
        "service": l.module,          // workflow | edge | presence | device — facet by service
        "ddtags": tags,
        "level": l.level,
        "message": l.message,
        "trace_id": l.trace_id.map(|t| t.to_string()),
        "entity_id": l.entity_id,
        "entity_type": l.entity_type,
        "payload": l.payload,
    })
}

// ---- sync path (sentinel-flow engine threads) --------------------------------

/// Insert a single log entry on the calling (blocking) thread. Errors are logged
/// and swallowed — log writes must never crash the caller.
pub fn insert_log_sync(db: &Db, log: PlatformLog) {
    // Card #145: ship to Datadog when configured, off Postgres.
    if crate::datadog::configured() {
        crate::datadog::ship_blocking(std::slice::from_ref(&to_dd_record(&log)));
        return;
    }
    let pool = db.pool();
    let now = crate::timeutil::now_rfc3339();
    let res: Result<(), sqlx::Error> = db.block(async move {
        sqlx::query(
            "INSERT INTO platform_logs \
             (tenant_id, module, trace_id, entity_id, entity_type, level, message, payload, created_at) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)",
        )
        .bind(log.tenant_id)
        .bind(log.module)
        .bind(log.trace_id)
        .bind(log.entity_id.as_deref())
        .bind(log.entity_type)
        .bind(log.level)
        .bind(&log.message)
        .bind(log.payload.as_ref())
        .bind(&now)
        .execute(&pool)
        .await?;
        Ok(())
    });
    if let Err(e) = res {
        log::warn!("platform_logs::insert_log_sync failed: {e}");
    }
}

/// Batch-insert log entries on the calling (blocking) thread. No-op on empty slice.
pub fn insert_logs_sync(db: &Db, logs: Vec<PlatformLog>) {
    if logs.is_empty() {
        return;
    }
    // Card #145: ship to Datadog when configured, off Postgres.
    if crate::datadog::configured() {
        let records: Vec<Value> = logs.iter().map(to_dd_record).collect();
        crate::datadog::ship_blocking(&records);
        return;
    }
    let pool = db.pool();
    let now = crate::timeutil::now_rfc3339();
    let res: Result<(), sqlx::Error> = db.block(async move {
        let mut qb = sqlx::QueryBuilder::new(
            "INSERT INTO platform_logs \
             (tenant_id, module, trace_id, entity_id, entity_type, level, message, payload, created_at) ",
        );
        qb.push_values(logs.iter(), |mut b, l| {
            b.push_bind(l.tenant_id)
                .push_bind(l.module)
                .push_bind(l.trace_id)
                .push_bind(l.entity_id.as_deref())
                .push_bind(l.entity_type)
                .push_bind(l.level)
                .push_bind(&l.message)
                .push_bind(l.payload.as_ref())
                .push_bind(&now);
        });
        qb.build().execute(&pool).await?;
        Ok(())
    });
    if let Err(e) = res {
        log::warn!("platform_logs::insert_logs_sync failed: {e}");
    }
}

// ---- async queue (sentinel-control) -----------------------------------------

/// Fire-and-forget log queue. `push` is sync-safe (try_send, never blocks).
/// The drain task batch-inserts every 500 ms (or when the batch reaches 200).
/// Drops entries silently when the channel is full — log loss is acceptable.
#[derive(Clone)]
pub struct LogQueue(tokio::sync::mpsc::Sender<PlatformLog>);

impl LogQueue {
    /// Spawn the drain task in the current tokio runtime and return the queue handle.
    /// Must be called from inside a tokio context (e.g. `rt.block_on(async { ... })`).
    pub fn spawn(pool: PgPool) -> Self {
        let (tx, rx) = tokio::sync::mpsc::channel(1_000);
        tokio::spawn(drain(rx, pool));
        Self(tx)
    }

    /// Non-blocking push. Silently drops if the channel is full.
    pub fn push(&self, log: PlatformLog) {
        let _ = self.0.try_send(log);
    }
}

async fn drain(mut rx: tokio::sync::mpsc::Receiver<PlatformLog>, pool: PgPool) {
    let mut batch: Vec<PlatformLog> = Vec::with_capacity(200);
    let mut ticker = tokio::time::interval(std::time::Duration::from_millis(500));
    loop {
        tokio::select! {
            msg = rx.recv() => {
                match msg {
                    Some(log) => {
                        batch.push(log);
                        if batch.len() >= 200 {
                            flush(&mut batch, &pool).await;
                        }
                    }
                    None => {
                        flush(&mut batch, &pool).await;
                        return;
                    }
                }
            }
            _ = ticker.tick() => {
                flush(&mut batch, &pool).await;
            }
        }
    }
}

async fn flush(batch: &mut Vec<PlatformLog>, pool: &PgPool) {
    if batch.is_empty() {
        return;
    }
    let to_insert: Vec<PlatformLog> = batch.drain(..).collect();
    // Card #145: ship to Datadog when configured, off Postgres. ureq is blocking, so hand the batch
    // to a blocking thread rather than stalling the drain task's runtime worker.
    if crate::datadog::configured() {
        let records: Vec<Value> = to_insert.iter().map(to_dd_record).collect();
        tokio::task::spawn_blocking(move || crate::datadog::ship_blocking(&records));
        return;
    }
    let now = crate::timeutil::now_rfc3339();
    let mut qb = sqlx::QueryBuilder::new(
        "INSERT INTO platform_logs \
         (tenant_id, module, trace_id, entity_id, entity_type, level, message, payload, created_at) ",
    );
    qb.push_values(to_insert.iter(), |mut b, l| {
        b.push_bind(l.tenant_id)
            .push_bind(l.module)
            .push_bind(l.trace_id)
            .push_bind(l.entity_id.as_deref())
            .push_bind(l.entity_type)
            .push_bind(l.level)
            .push_bind(&l.message)
            .push_bind(l.payload.as_ref())
            .push_bind(&now);
    });
    if let Err(e) = qb.build().execute(pool).await {
        log::warn!("platform_logs::flush failed: {e}");
    }
}

// ---- query (async, for the control-plane API) --------------------------------

/// One row returned from [`list_logs`].
#[derive(serde::Serialize)]
pub struct LogRow {
    pub id:          i64,
    pub module:      String,
    pub trace_id:    Option<Uuid>,
    pub entity_id:   Option<String>,
    pub entity_type: Option<String>,
    pub level:       String,
    pub message:     String,
    pub payload:     Option<Value>,
    pub created_at:  String,
}

/// Filters + cursor for [`list_logs`].
pub struct LogQuery {
    pub module:      Option<String>,
    pub trace_id:    Option<Uuid>,
    pub entity_type: Option<String>,
    pub entity_id:   Option<String>,
    pub level:       Option<String>,
    pub from:        Option<String>,  // RFC3339 lower bound (created_at >=)
    pub to:          Option<String>,  // RFC3339 upper bound (created_at <=)
    pub after:       Option<i64>,     // cursor: return rows with id < after (newest-first)
    pub limit:       i64,
}

/// Paginated log query, newest-first. `limit` is clamped to 1–200.
pub async fn list_logs(
    pool: &PgPool,
    tenant_id: Uuid,
    q: LogQuery,
) -> sqlx::Result<Vec<LogRow>> {
    let limit = q.limit.clamp(1, 200);
    let mut qb = sqlx::QueryBuilder::new(
        "SELECT id, module, trace_id, entity_id, entity_type, level, message, payload, created_at \
         FROM platform_logs WHERE tenant_id = ",
    );
    qb.push_bind(tenant_id);
    if let Some(m) = &q.module {
        qb.push(" AND module = ").push_bind(m.clone());
    }
    if let Some(tid) = q.trace_id {
        qb.push(" AND trace_id = ").push_bind(tid);
    }
    if let Some(et) = &q.entity_type {
        qb.push(" AND entity_type = ").push_bind(et.clone());
    }
    if let Some(eid) = &q.entity_id {
        qb.push(" AND entity_id = ").push_bind(eid.clone());
    }
    if let Some(lv) = &q.level {
        qb.push(" AND level = ").push_bind(lv.clone());
    }
    if let Some(from) = &q.from {
        qb.push(" AND created_at >= ").push_bind(from.clone());
    }
    if let Some(to) = &q.to {
        qb.push(" AND created_at <= ").push_bind(to.clone());
    }
    if let Some(after) = q.after {
        qb.push(" AND id < ").push_bind(after);
    }
    qb.push(" ORDER BY id DESC LIMIT ").push_bind(limit);

    type Row = (i64, String, Option<Uuid>, Option<String>, Option<String>, String, String, Option<Value>, String);
    let rows = qb
        .build_query_as::<Row>()
        .fetch_all(pool)
        .await?;

    Ok(rows
        .into_iter()
        .map(|(id, module, trace_id, entity_id, entity_type, level, message, payload, created_at)| {
            LogRow { id, module, trace_id, entity_id, entity_type, level, message, payload, created_at }
        })
        .collect())
}
