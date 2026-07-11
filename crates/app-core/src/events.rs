//! Agnostic event stream (card #66) — the unified, user-facing, plan-retained feed every
//! subsystem registers into. Mirrors [`crate::platform_logs`]'s shape (that is the low-level debug
//! trace; this is the product timeline):
//!
//! - [`insert_event_sync`] / [`insert_events_sync`]: blocking (`Db::block`), for std-thread callers
//!   (the flow engine).
//! - [`EventQueue`]: async, fire-and-forget mpsc → batched insert, for control's async handlers.
//! - [`list_events`]: async, cursor-paginated + filtered — the read path the feed consumes.
//!
//! An event carries entity refs (camera / clip_id / room / device / graph / user) so a future Live
//! "time-travel" timeline can place it at the right playback moment. Producers set `occurred_at` to
//! the precise moment; `created_at` is stamped at write.

use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

use crate::timeutil::now_rfc3339;
use crate::db::Db;

/// A new occurrence to record. Build with [`NewEvent::new`] then set the fields that apply;
/// everything but `kind`/`source` is optional. `occurred_at` defaults to now — override it when
/// the event describes a moment other than write time.
#[derive(Debug, Clone, Default)]
pub struct NewEvent {
    pub tenant_id:   Uuid,
    pub kind:        String,        // catalog slug
    pub source:      String,        // system | user | workflow | edge
    pub severity:    String,        // debug | info | warn | error
    pub title:       Option<String>,
    pub message:     Option<String>,
    pub camera:      Option<String>,
    pub clip_id:     Option<String>,
    pub room_id:     Option<i64>,
    pub room_name:   Option<String>,
    pub device_id:   Option<String>,
    pub graph_id:    Option<i64>,
    pub user_id:     Option<Uuid>,
    pub data:        Option<Value>,
    pub occurred_at: String,        // RFC3339
}

impl NewEvent {
    /// A minimal event: tenant + kind + source, severity `info`, occurred now. Set other fields
    /// on the returned value.
    pub fn new(tenant_id: Uuid, kind: impl Into<String>, source: impl Into<String>) -> Self {
        NewEvent {
            tenant_id,
            kind: kind.into(),
            source: source.into(),
            severity: "info".to_string(),
            occurred_at: now_rfc3339(),
            ..Default::default()
        }
    }
}

/// One known event kind, for the feed's filter. A code catalog (like `repo::system_events()`),
/// not a table — kinds are defined where they're produced.
pub struct KindInfo {
    pub kind:  &'static str,
    pub label: &'static str,
}

/// The kinds the feed knows how to label. Additive — an unknown kind still stores and lists fine.
pub fn catalog() -> &'static [KindInfo] {
    &[
        KindInfo { kind: "alert",           label: "Alert" },
        KindInfo { kind: "presence",        label: "Presence" },
        KindInfo { kind: "clip.started",    label: "Clip started" },
        KindInfo { kind: "clip.stopped",    label: "Clip stopped" },
        KindInfo { kind: "workflow.custom", label: "Workflow event" },
        KindInfo { kind: "auth.login",      label: "Login" },
    ]
}

// ---- sync path (flow engine threads) ----------------------------------------

/// Insert one event on the calling (blocking) thread. Errors are logged and swallowed — recording
/// an event must never crash the caller.
pub fn insert_event_sync(db: &Db, ev: NewEvent) {
    insert_events_sync(db, vec![ev]);
}

/// Batch-insert events on the calling (blocking) thread. No-op on empty.
pub fn insert_events_sync(db: &Db, events: Vec<NewEvent>) {
    if events.is_empty() {
        return;
    }
    let pool = db.pool();
    if let Err(e) = db.block(async move { insert_batch(&pool, &events).await }) {
        log::warn!("events::insert_events_sync failed: {e}");
    }
}

// ---- async queue (control) --------------------------------------------------

/// Fire-and-forget event queue. `push` never blocks (try_send); the drain batch-inserts every
/// 500 ms or at 200 rows. Drops when the channel is full — the same acceptable-loss contract as the
/// platform-log queue.
#[derive(Clone)]
pub struct EventQueue(tokio::sync::mpsc::Sender<NewEvent>);

impl EventQueue {
    /// Spawn the drain task in the current tokio runtime and return the handle.
    pub fn spawn(pool: PgPool) -> Self {
        let (tx, rx) = tokio::sync::mpsc::channel(1_000);
        tokio::spawn(drain(rx, pool));
        Self(tx)
    }

    /// Non-blocking push. Silently drops if the channel is full.
    pub fn push(&self, ev: NewEvent) {
        let _ = self.0.try_send(ev);
    }
}

async fn drain(mut rx: tokio::sync::mpsc::Receiver<NewEvent>, pool: PgPool) {
    let mut batch: Vec<NewEvent> = Vec::with_capacity(200);
    let mut ticker = tokio::time::interval(std::time::Duration::from_millis(500));
    loop {
        tokio::select! {
            msg = rx.recv() => match msg {
                Some(ev) => {
                    batch.push(ev);
                    if batch.len() >= 200 {
                        flush(&mut batch, &pool).await;
                    }
                }
                None => { flush(&mut batch, &pool).await; return; }
            },
            _ = ticker.tick() => flush(&mut batch, &pool).await,
        }
    }
}

async fn flush(batch: &mut Vec<NewEvent>, pool: &PgPool) {
    if batch.is_empty() {
        return;
    }
    let events: Vec<NewEvent> = batch.drain(..).collect();
    if let Err(e) = insert_batch(pool, &events).await {
        log::warn!("events::flush failed: {e}");
    }
}

/// The one INSERT both paths share. `created_at` is stamped here.
async fn insert_batch(pool: &PgPool, events: &[NewEvent]) -> sqlx::Result<()> {
    let now = now_rfc3339();
    let mut qb = sqlx::QueryBuilder::new(
        "INSERT INTO events \
         (tenant_id, kind, source, severity, title, message, camera, clip_id, room_id, room_name, \
          device_id, graph_id, user_id, data, occurred_at, created_at) ",
    );
    qb.push_values(events.iter(), |mut b, e| {
        b.push_bind(e.tenant_id)
            .push_bind(&e.kind)
            .push_bind(&e.source)
            .push_bind(if e.severity.is_empty() { "info" } else { &e.severity })
            .push_bind(e.title.as_deref())
            .push_bind(e.message.as_deref())
            .push_bind(e.camera.as_deref())
            .push_bind(e.clip_id.as_deref())
            .push_bind(e.room_id)
            .push_bind(e.room_name.as_deref())
            .push_bind(e.device_id.as_deref())
            .push_bind(e.graph_id)
            .push_bind(e.user_id)
            .push_bind(e.data.as_ref())
            .push_bind(&e.occurred_at)
            .push_bind(&now);
    });
    qb.build().execute(pool).await.map(|_| ())
}

// ---- query (async, for the feed API) ----------------------------------------

/// One row from [`list_events`].
#[derive(serde::Serialize, sqlx::FromRow)]
pub struct EventRow {
    pub id:          i64,
    pub kind:        String,
    pub source:      String,
    pub severity:    String,
    pub title:       Option<String>,
    pub message:     Option<String>,
    pub camera:      Option<String>,
    pub clip_id:     Option<String>,
    pub room_id:     Option<i64>,
    pub room_name:   Option<String>,
    pub device_id:   Option<String>,
    pub graph_id:    Option<i64>,
    pub user_id:     Option<Uuid>,
    pub data:        Option<Value>,
    pub occurred_at: String,
    pub created_at:  String,
}

/// Filters + cursor for [`list_events`].
#[derive(Default)]
pub struct EventQuery {
    pub kind:     Option<String>,
    pub source:   Option<String>,
    pub severity: Option<String>,
    pub camera:   Option<String>,
    pub from:     Option<String>,  // occurred_at >= (RFC3339)
    pub to:       Option<String>,  // occurred_at <= (RFC3339)
    pub after:    Option<i64>,     // cursor: rows with id < after (newest-first)
    pub limit:    i64,
}

/// Paginated event query, newest-first. `limit` clamped 1–200.
pub async fn list_events(pool: &PgPool, tenant_id: Uuid, q: EventQuery) -> sqlx::Result<Vec<EventRow>> {
    let limit = q.limit.clamp(1, 200);
    let mut qb = sqlx::QueryBuilder::new(
        "SELECT id, kind, source, severity, title, message, camera, clip_id, room_id, room_name, \
                device_id, graph_id, user_id, data, occurred_at, created_at \
         FROM events WHERE tenant_id = ",
    );
    qb.push_bind(tenant_id);
    if let Some(k) = &q.kind {
        qb.push(" AND kind = ").push_bind(k.clone());
    }
    if let Some(s) = &q.source {
        qb.push(" AND source = ").push_bind(s.clone());
    }
    if let Some(sv) = &q.severity {
        qb.push(" AND severity = ").push_bind(sv.clone());
    }
    if let Some(cam) = &q.camera {
        qb.push(" AND camera = ").push_bind(cam.clone());
    }
    if let Some(from) = &q.from {
        qb.push(" AND occurred_at >= ").push_bind(from.clone());
    }
    if let Some(to) = &q.to {
        qb.push(" AND occurred_at <= ").push_bind(to.clone());
    }
    if let Some(after) = q.after {
        qb.push(" AND id < ").push_bind(after);
    }
    qb.push(" ORDER BY id DESC LIMIT ").push_bind(limit);
    qb.build_query_as::<EventRow>().fetch_all(pool).await
}
