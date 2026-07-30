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

use crate::db::Db;
use crate::timeutil::now_rfc3339;

/// Severity of an event, low → high. Relocated from the retired `alerts` module — the alert
/// workflow nodes pick a level here, which maps to the event's `severity` string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AlertLevel {
    Debug,
    Info,
    Warn,
    Error,
}

impl AlertLevel {
    /// Ascending severity — the order the CreateAlert node's level dropdown offers.
    pub const ALL: [AlertLevel; 4] = [
        AlertLevel::Debug,
        AlertLevel::Info,
        AlertLevel::Warn,
        AlertLevel::Error,
    ];

    pub fn label(self) -> &'static str {
        match self {
            AlertLevel::Debug => "DEBUG",
            AlertLevel::Info => "INFO",
            AlertLevel::Warn => "WARN",
            AlertLevel::Error => "ERROR",
        }
    }

    /// The `events.severity` string this level stores as.
    pub fn severity(self) -> &'static str {
        match self {
            AlertLevel::Debug => "debug",
            AlertLevel::Info => "info",
            AlertLevel::Warn => "warn",
            AlertLevel::Error => "error",
        }
    }

    /// Parse a level name (case-insensitive).
    pub fn parse(s: &str) -> Option<AlertLevel> {
        match s.trim().to_ascii_lowercase().as_str() {
            "debug" => Some(AlertLevel::Debug),
            "info" => Some(AlertLevel::Info),
            "warn" | "warning" => Some(AlertLevel::Warn),
            "error" | "err" => Some(AlertLevel::Error),
            _ => None,
        }
    }
}

impl Default for AlertLevel {
    fn default() -> Self {
        AlertLevel::Info
    }
}

/// A new occurrence to record. Build with [`NewEvent::new`] then set the fields that apply;
/// everything but `kind`/`source` is optional. `occurred_at` defaults to now — override it when
/// the event describes a moment other than write time.
#[derive(Debug, Clone, Default)]
pub struct NewEvent {
    pub tenant_id: Uuid,
    pub kind: String,     // catalog slug
    pub source: String,   // system | user | workflow | edge
    pub severity: String, // debug | info | warn | error
    pub title: Option<String>,
    pub message: Option<String>,
    pub camera: Option<String>,
    pub clip_id: Option<String>,
    pub room_id: Option<i64>,
    pub room_name: Option<String>,
    pub device_id: Option<String>,
    pub graph_id: Option<i64>,
    pub user_id: Option<Uuid>,
    pub data: Option<Value>,
    pub occurred_at: String, // RFC3339
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
    pub kind: &'static str,
    pub label: &'static str,
}

/// The kinds the feed knows how to label. Additive — an unknown kind still stores and lists fine.
pub fn catalog() -> &'static [KindInfo] {
    &[
        KindInfo {
            kind: "alert",
            label: "Alert",
        },
        KindInfo {
            kind: "presence",
            label: "Presence",
        },
        KindInfo {
            kind: "clip.started",
            label: "Clip started",
        },
        KindInfo {
            kind: "clip.stopped",
            label: "Clip stopped",
        },
        KindInfo {
            kind: "workflow.custom",
            label: "Workflow event",
        },
        KindInfo {
            kind: "auth.login",
            label: "Login",
        },
        // The failure side of sign-in. `auth.login_failed` is bounded by the lockout that follows
        // it (attempts past the limit never reach the producer), so it cannot be used to flood a
        // tenant's feed; attempts against an already-locked account are deliberately log-only for
        // exactly that reason. See `handlers::login`.
        KindInfo {
            kind: "auth.login_failed",
            label: "Failed sign-in",
        },
        KindInfo {
            kind: "auth.account_locked",
            label: "Account locked",
        },
        KindInfo {
            kind: "auth.password_reset",
            label: "Password reset",
        },
        KindInfo {
            kind: "auth.user_removed",
            label: "User removed",
        },
        KindInfo {
            kind: "auth.ownership_transferred",
            label: "Ownership transferred",
        },
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

/// Insert one event and return its new id — for producers that chain the id (a workflow
/// CreateAlert whose id feeds a DeliverAlert). Blocking; logs and returns `None` on error.
pub fn insert_event_returning_sync(db: &Db, ev: NewEvent) -> Option<i64> {
    let pool = db.pool();
    let now = now_rfc3339();
    db.block(async move {
        sqlx::query_scalar::<_, i64>(
            "INSERT INTO events \
             (tenant_id, kind, source, severity, title, message, camera, clip_id, room_id, room_name, \
              device_id, graph_id, user_id, data, occurred_at, created_at) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16) RETURNING id",
        )
        .bind(ev.tenant_id)
        .bind(&ev.kind).bind(&ev.source)
        .bind(if ev.severity.is_empty() { "info" } else { &ev.severity })
        .bind(ev.title.as_deref()).bind(ev.message.as_deref())
        .bind(ev.camera.as_deref()).bind(ev.clip_id.as_deref())
        .bind(ev.room_id).bind(ev.room_name.as_deref())
        .bind(ev.device_id.as_deref()).bind(ev.graph_id).bind(ev.user_id)
        .bind(ev.data.as_ref()).bind(&ev.occurred_at).bind(&now)
        .fetch_one(&pool).await
    })
    .map_err(|e| log::warn!("events::insert_event_returning_sync failed: {e}"))
    .ok()
}

// ---- sync reads (workflow nodes / agent on flow engine threads) --------------

const SELECT_COLS: &str = "id, kind, source, severity, title, message, camera, clip_id, room_id, \
    room_name, device_id, graph_id, user_id, data, occurred_at, created_at";

/// One event by id, tenant-scoped. Blocking.
pub fn event_by_id_sync(db: &Db, tenant_id: Uuid, id: i64) -> Option<EventRow> {
    let pool = db.pool();
    db.block(async move {
        sqlx::query_as::<_, EventRow>(&format!(
            "SELECT {SELECT_COLS} FROM events WHERE tenant_id = $1 AND id = $2"
        ))
        .bind(tenant_id)
        .bind(id)
        .fetch_optional(&pool)
        .await
        .ok()
        .flatten()
    })
}

/// The newest event of a kind for a tenant. Blocking. (GetLastAlert → kind='alert'.)
pub fn latest_of_kind_sync(db: &Db, tenant_id: Uuid, kind: &str) -> Option<EventRow> {
    let pool = db.pool();
    let kind = kind.to_string();
    db.block(async move {
        sqlx::query_as::<_, EventRow>(&format!("SELECT {SELECT_COLS} FROM events WHERE tenant_id = $1 AND kind = $2 ORDER BY id DESC LIMIT 1"))
            .bind(tenant_id).bind(&kind).fetch_optional(&pool).await.ok().flatten()
    })
}

/// A kind's events, newest first, optionally since a timestamp. Blocking. (Agent's alert list /
/// daily summary.) `since` = an RFC3339 lower bound on `occurred_at`; `None` = the full log.
pub fn list_kind_sync(
    db: &Db,
    tenant_id: Uuid,
    kind: &str,
    since: Option<String>,
    limit: i64,
) -> Vec<EventRow> {
    let pool = db.pool();
    let kind = kind.to_string();
    db.block(async move {
        let mut qb = sqlx::QueryBuilder::new(format!(
            "SELECT {SELECT_COLS} FROM events WHERE tenant_id = "
        ));
        qb.push_bind(tenant_id).push(" AND kind = ").push_bind(kind);
        if let Some(s) = since {
            qb.push(" AND occurred_at >= ").push_bind(s);
        }
        qb.push(" ORDER BY id DESC LIMIT ")
            .push_bind(limit.clamp(1, 1000));
        qb.build_query_as::<EventRow>()
            .fetch_all(&pool)
            .await
            .unwrap_or_default()
    })
}

/// Local start-of-day as an RFC3339 lower bound, for "today's" filters.
pub fn today_start_rfc3339() -> String {
    format!("{}T00:00:00", &crate::timeutil::now_rfc3339()[..10])
}

/// Whether an alert event's `data.deliveries` records at least one successful beacon delivery.
pub fn event_delivered(e: &EventRow) -> bool {
    e.data
        .as_ref()
        .and_then(|d| d.get("deliveries"))
        .and_then(|d| d.as_array())
        .map(|arr| {
            arr.iter()
                .any(|x| x.get("ok").and_then(|v| v.as_bool()).unwrap_or(false))
        })
        .unwrap_or(false)
}

/// Replace an event's `data` (used to record beacon deliveries on an alert event). Blocking.
pub fn set_data_sync(db: &Db, tenant_id: Uuid, id: i64, data: Value) {
    let pool = db.pool();
    let res: Result<_, sqlx::Error> = db.block(async move {
        sqlx::query("UPDATE events SET data = $3 WHERE tenant_id = $1 AND id = $2")
            .bind(tenant_id)
            .bind(id)
            .bind(&data)
            .execute(&pool)
            .await
    });
    if let Err(e) = res {
        log::warn!("events::set_data_sync failed: {e}");
    }
}

/// Delete an event by id, tenant-scoped. Blocking. Returns whether a row was removed.
pub fn delete_event_sync(db: &Db, tenant_id: Uuid, id: i64) -> bool {
    let pool = db.pool();
    db.block(async move {
        sqlx::query("DELETE FROM events WHERE tenant_id = $1 AND id = $2")
            .bind(tenant_id)
            .bind(id)
            .execute(&pool)
            .await
            .map(|r| r.rows_affected() > 0)
            .unwrap_or(false)
    })
}

/// Deliver an alert event to every enabled beacon, recording the per-beacon result in the event's
/// `data.deliveries`. The one delivery path — shared by the DeliverAlert workflow node, the control
/// `/alerts/:id/deliver` endpoint, and the chat agent's deliver tool (replaces `alerts::deliver`).
/// Network IO is inline, so callers run it off a background thread.
pub fn deliver_event_to_beacons(
    db: &Db,
    tenant_id: Uuid,
    id: i64,
    notif: &crate::notification::NotificationStore,
    cfg: &crate::notification::NotificationConfig,
) -> Result<String, String> {
    let ev = event_by_id_sync(db, tenant_id, id).ok_or_else(|| format!("Alert {id} not found."))?;
    let loc = ev.room_name.as_deref().unwrap_or("site");
    let msg = format!(
        "[{}] {} \u{2014} {}",
        ev.severity.to_uppercase(),
        loc,
        ev.message.clone().unwrap_or_default()
    );

    let beacons: Vec<crate::notification::NotificationTarget> = {
        notif
            .lock()
            .unwrap()
            .iter()
            .filter(|t| t.enabled)
            .cloned()
            .collect()
    };
    if beacons.is_empty() {
        return Err("No live beacons to deliver to.".into());
    }
    let mut deliveries = Vec::new();
    let mut ok_count = 0usize;
    for b in &beacons {
        let ok = crate::notification::dispatch(notif, cfg, Some(&b.label), &msg, None).is_ok();
        if ok {
            ok_count += 1;
        }
        deliveries.push(serde_json::json!({ "target": b.label, "chat_id": b.chat_id, "at": now_rfc3339(), "ok": ok }));
    }
    set_data_sync(
        db,
        tenant_id,
        id,
        serde_json::json!({ "deliveries": deliveries }),
    );
    Ok(format!(
        "Delivered alert {id} to {ok_count}/{} beacon(s).",
        beacons.len()
    ))
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
            .push_bind(if e.severity.is_empty() {
                "info"
            } else {
                &e.severity
            })
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
    pub id: i64,
    pub kind: String,
    pub source: String,
    pub severity: String,
    pub title: Option<String>,
    pub message: Option<String>,
    pub camera: Option<String>,
    pub clip_id: Option<String>,
    pub room_id: Option<i64>,
    pub room_name: Option<String>,
    pub device_id: Option<String>,
    pub graph_id: Option<i64>,
    pub user_id: Option<Uuid>,
    pub data: Option<Value>,
    pub occurred_at: String,
    pub created_at: String,
}

/// Filters + cursor for [`list_events`].
#[derive(Default)]
pub struct EventQuery {
    pub kind: Option<String>,
    pub source: Option<String>,
    pub severity: Option<String>,
    pub camera: Option<String>,
    pub from: Option<String>, // occurred_at >= (RFC3339)
    pub to: Option<String>,   // occurred_at <= (RFC3339)
    pub after: Option<i64>,   // cursor: rows with id < after (newest-first)
    pub limit: i64,
}

/// Paginated event query, newest-first. `limit` clamped 1–200.
pub async fn list_events(
    pool: &PgPool,
    tenant_id: Uuid,
    q: EventQuery,
) -> sqlx::Result<Vec<EventRow>> {
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
    // Compare as instants, not strings: `occurred_at` is RFC3339 with an offset, and the window
    // bounds arrive as UTC instants — a naive string `>=` drops events whose textual offset differs
    // from the bound's (the same timezone bug that hid recent clips from Playback). The cast happens
    // in Postgres, so no timestamptz is decoded into Rust.
    if let Some(from) = &q.from {
        qb.push(" AND occurred_at::timestamptz >= ")
            .push_bind(from.clone())
            .push("::timestamptz");
    }
    if let Some(to) = &q.to {
        qb.push(" AND occurred_at::timestamptz <= ")
            .push_bind(to.clone())
            .push("::timestamptz");
    }
    if let Some(after) = q.after {
        qb.push(" AND id < ").push_bind(after);
    }
    qb.push(" ORDER BY id DESC LIMIT ").push_bind(limit);
    qb.build_query_as::<EventRow>().fetch_all(pool).await
}
