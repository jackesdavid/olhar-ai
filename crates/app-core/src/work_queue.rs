//! Generic durable work queue (card #64). One table, `work_jobs`, for asynchronous jobs across
//! the system: enqueue, claim, retry, and monitor from a single place. Jobs are grouped by
//! `category` (the domain) and `kind` (the action), keyed to a `request_id` so the several jobs
//! of one request can be tracked together.
//!
//! **Atomic claim.** `claim` locks a due job, flips it to `running`, and hands it to exactly one
//! consumer (`FOR UPDATE SKIP LOCKED`). This lets non-idempotent work run on the queue safely —
//! firing a workflow or sending a Telegram must not double-run. A `running` job whose consumer
//! crashed is reaped: the claim also picks up `running` rows whose `updated_at` is older than
//! [`STALE_SECS`], so a crash self-heals on the next pass. `run_after` schedules retries (backoff)
//! instead of re-running immediately.
//!
//! **Two calling styles.** The async fns take `&PgPool` (control, which has one, uses these and
//! also gets `LISTEN`/`NOTIFY` latency). The `*_blocking` wrappers take `&Db` for the services
//! that run consumers on std threads with `Db::block` (flow, notification) and have no `PgPool`.

use serde::Serialize;
use sqlx::PgPool;
use uuid::Uuid;

use crate::db::Db;
use crate::timeutil::now_rfc3339;

/// The `NOTIFY`/`LISTEN` channel every consumer watches. Payload is the job's `category`.
pub const CHANNEL: &str = "work_jobs";

/// A `running` job untouched for this long is presumed abandoned (consumer crashed) and re-claimed.
/// Generous — far longer than any legitimate handler — so a slow job is never stolen mid-flight.
const STALE_SECS: i64 = 600;

/// A claimed job. `payload` is self-contained — the consumer needs nothing else to act.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Job {
    pub id: Uuid,
    pub request_id: Uuid,
    pub tenant_id: Uuid,
    pub category: String,
    pub kind: String,
    pub payload: serde_json::Value,
    pub attempts: i32,
}

/// One job to enqueue: its action (`kind`) and self-contained data.
pub struct NewJob {
    pub kind: String,
    pub payload: serde_json::Value,
}

/// Enqueue all jobs of one request in a single transaction, then wake consumers with one NOTIFY.
/// Atomic: either the whole request is queued or none of it is.
pub async fn enqueue(
    pool: &PgPool,
    request_id: Uuid,
    tenant_id: Uuid,
    category: &str,
    jobs: &[NewJob],
) -> sqlx::Result<()> {
    let now = now_rfc3339();
    let mut tx = pool.begin().await?;
    for j in jobs {
        // run_after = created_at → immediately due.
        sqlx::query(
            "INSERT INTO work_jobs (request_id, tenant_id, category, kind, payload, created_at, updated_at, run_after)
             VALUES ($1, $2, $3, $4, $5, $6, $6, $6)",
        )
        .bind(request_id).bind(tenant_id).bind(category).bind(&j.kind).bind(&j.payload).bind(&now)
        .execute(&mut *tx).await?;
    }
    // NOTIFY inside the tx so it fires exactly when the rows commit (Postgres holds notifications
    // until commit and drops them on rollback).
    sqlx::query("SELECT pg_notify($1, $2)")
        .bind(CHANNEL)
        .bind(category)
        .execute(&mut *tx)
        .await?;
    tx.commit().await
}

/// Enqueue one job that only becomes due at `run_after` (RFC3339) — a durable timer.
///
/// [`enqueue`] always writes `run_after = created_at`, i.e. due immediately. This binds the
/// caller's own moment instead, which [`claim`] already honours (it only takes rows whose
/// `run_after <= now`). Used for `DelayTrigger`'s trailing edge: the stop is a row in Postgres,
/// so it survives a pod restart rather than dying with a held thread. No `NOTIFY` — waking a
/// consumer for a job that isn't due yet only burns a poll.
pub async fn enqueue_at(
    pool: &PgPool,
    request_id: Uuid,
    tenant_id: Uuid,
    category: &str,
    job: &NewJob,
    run_after: &str,
) -> sqlx::Result<()> {
    let now = now_rfc3339();
    sqlx::query(
        "INSERT INTO work_jobs (request_id, tenant_id, category, kind, payload, created_at, updated_at, run_after)
         VALUES ($1, $2, $3, $4, $5, $6, $6, $7)",
    )
    .bind(request_id).bind(tenant_id).bind(category).bind(&job.kind).bind(&job.payload).bind(&now).bind(run_after)
    .execute(pool).await.map(|_| ())
}

/// Move a claimed job's next attempt to an explicit moment and return it to `pending`. Like
/// [`release`], but the caller names the time rather than a delay — for a timer whose deadline
/// moved (a debounce window that got extended).
pub async fn reschedule(pool: &PgPool, id: Uuid, run_after: &str) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE work_jobs SET status = 'pending', run_after = $2, updated_at = $3 WHERE id = $1",
    )
    .bind(id)
    .bind(run_after)
    .bind(now_rfc3339())
    .execute(pool)
    .await
    .map(|_| ())
}

/// Atomically claim up to `limit` jobs of a (category, kind): due `pending` rows, plus stale
/// `running` rows (reaped), locked with `FOR UPDATE SKIP LOCKED` and flipped to `running` in the
/// same transaction. Exactly-once handoff — a claimed job is invisible to other consumers until it
/// is marked terminal or goes stale.
pub async fn claim(
    pool: &PgPool,
    category: &str,
    kind: &str,
    limit: i64,
) -> sqlx::Result<Vec<Job>> {
    let now = now_rfc3339();
    let stale_before = (chrono::Local::now() - chrono::Duration::seconds(STALE_SECS)).to_rfc3339();
    let mut tx = pool.begin().await?;
    let jobs = sqlx::query_as::<_, Job>(
        "SELECT id, request_id, tenant_id, category, kind, payload, attempts
         FROM work_jobs
         WHERE category = $1 AND kind = $2
           AND ( (status = 'pending' AND run_after <= $3)
              OR (status = 'running' AND updated_at < $4) )
         ORDER BY run_after
         LIMIT $5
         FOR UPDATE SKIP LOCKED",
    )
    .bind(category)
    .bind(kind)
    .bind(&now)
    .bind(&stale_before)
    .bind(limit)
    .fetch_all(&mut *tx)
    .await?;

    if !jobs.is_empty() {
        let ids: Vec<Uuid> = jobs.iter().map(|j| j.id).collect();
        sqlx::query(
            "UPDATE work_jobs SET status = 'running', attempts = attempts + 1, updated_at = $2
             WHERE id = ANY($1)",
        )
        .bind(&ids)
        .bind(&now)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    // Reflect the attempt bump the UPDATE just made, so a handler sees its real attempt count.
    Ok(jobs
        .into_iter()
        .map(|mut j| {
            j.attempts += 1;
            j
        })
        .collect())
}

/// Mark a job done. Terminal.
pub async fn mark_done(pool: &PgPool, id: Uuid) -> sqlx::Result<()> {
    sqlx::query("UPDATE work_jobs SET status = 'done', updated_at = $2 WHERE id = $1")
        .bind(id)
        .bind(now_rfc3339())
        .execute(pool)
        .await
        .map(|_| ())
}

/// Record a failure: keep it `pending` for retry until `max_attempts`, then park it `failed` so a
/// poisoned job can't spin forever. `attempts` was already bumped at claim, so compare it directly.
pub async fn mark_failed(
    pool: &PgPool,
    id: Uuid,
    err: &str,
    max_attempts: i32,
) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE work_jobs
         SET last_error = $2,
             status = CASE WHEN attempts >= $3 THEN 'failed' ELSE 'pending' END,
             updated_at = $4
         WHERE id = $1",
    )
    .bind(id)
    .bind(err)
    .bind(max_attempts)
    .bind(now_rfc3339())
    .execute(pool)
    .await
    .map(|_| ())
}

/// Return a claimed job to `pending` without counting it as a failure — for "not ready yet" (a
/// dependency offline). `delay_secs` holds it off the claim for a beat so it doesn't busy-loop.
pub async fn release(pool: &PgPool, id: Uuid, delay_secs: i64) -> sqlx::Result<()> {
    let run_after = (chrono::Local::now() + chrono::Duration::seconds(delay_secs)).to_rfc3339();
    sqlx::query(
        "UPDATE work_jobs SET status = 'pending', run_after = $2, updated_at = $3 WHERE id = $1",
    )
    .bind(id)
    .bind(&run_after)
    .bind(now_rfc3339())
    .execute(pool)
    .await
    .map(|_| ())
}

/// Requeue a failed attempt with a backoff delay (records the error). For non-idempotent work that
/// retries on transient errors — the notification send path (Phase 2). Distinct from `mark_failed`,
/// which parks permanently; the caller decides when a job is out of retries.
pub async fn retry_after(
    pool: &PgPool,
    id: Uuid,
    backoff_secs: i64,
    err: &str,
) -> sqlx::Result<()> {
    let run_after = (chrono::Local::now() + chrono::Duration::seconds(backoff_secs)).to_rfc3339();
    sqlx::query("UPDATE work_jobs SET status = 'pending', run_after = $2, last_error = $3, updated_at = $4 WHERE id = $1")
        .bind(id).bind(&run_after).bind(err).bind(now_rfc3339())
        .execute(pool).await.map(|_| ())
}

/// Summary of a request's jobs by status — the "did this finish?" query. A request is complete
/// when nothing is left `pending`/`running`; anything `failed` is a leg that needs attention.
pub async fn request_status(
    pool: &PgPool,
    request_id: Uuid,
) -> sqlx::Result<Vec<(String, String, i64)>> {
    sqlx::query_as::<_, (String, String, i64)>(
        "SELECT kind, status, COUNT(*) FROM work_jobs
         WHERE request_id = $1 GROUP BY kind, status",
    )
    .bind(request_id)
    .fetch_all(pool)
    .await
}

/// The `recording_id`s a tenant has an open (non-done) job for — the list marks these clips as
/// deletion-in-progress. Reads the JSONB payload, so it works for any category that stamps one.
pub async fn open_recording_ids(pool: &PgPool, tenant_id: Uuid) -> sqlx::Result<Vec<i64>> {
    let ids: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT payload->>'recording_id' FROM work_jobs
         WHERE tenant_id = $1 AND status <> 'done' AND payload ? 'recording_id'",
    )
    .bind(tenant_id)
    .fetch_all(pool)
    .await?;
    Ok(ids
        .into_iter()
        .filter_map(|s| s.parse::<i64>().ok())
        .collect())
}

// ---- blocking wrappers (for std-thread consumers with a `Db`, no PgPool) ----
// These live here in app_core so they can reach `Db::pool()` (pub(crate)); a service on a std
// thread calls them through its `Db` exactly like it called the old queue modules.

/// Enqueue one job (the common case for single-message categories like `workflow_event`).
pub fn enqueue_one_blocking(
    db: &Db,
    tenant_id: Uuid,
    category: &str,
    kind: &str,
    payload: serde_json::Value,
) {
    let pool = db.pool();
    let request_id = Uuid::new_v4();
    let category = category.to_string();
    let jobs = [NewJob {
        kind: kind.to_string(),
        payload,
    }];
    let res =
        db.block(async move { enqueue(&pool, request_id, tenant_id, &category, &jobs).await });
    if let Err(e) = res {
        log::error!("work_queue::enqueue_one_blocking failed: {e}");
    }
}

/// Enqueue one job due at `run_after` (RFC3339), blocking. See [`enqueue_at`].
pub fn enqueue_at_blocking(
    db: &Db,
    tenant_id: Uuid,
    category: &str,
    kind: &str,
    payload: serde_json::Value,
    run_after: &str,
) {
    let pool = db.pool();
    let request_id = Uuid::new_v4();
    let category = category.to_string();
    let job = NewJob {
        kind: kind.to_string(),
        payload,
    };
    let run_after = run_after.to_string();
    let res = db.block(async move {
        enqueue_at(&pool, request_id, tenant_id, &category, &job, &run_after).await
    });
    if let Err(e) = res {
        log::error!("work_queue::enqueue_at_blocking failed: {e}");
    }
}

/// Delete all still-pending jobs of a category (blocking). Used to reconcile schedule jobs (#111)
/// on graph reload: clear the category, then re-enqueue from the current enabled graphs. Only
/// `pending` rows are removed — a `running`/`done` job is never yanked out from under a consumer.
pub fn delete_pending_by_category_blocking(db: &Db, category: &str) {
    let pool = db.pool();
    let category = category.to_string();
    let res: Result<u64, sqlx::Error> = db.block(async move {
        let r = sqlx::query("DELETE FROM work_jobs WHERE category = $1 AND status = 'pending'")
            .bind(&category)
            .execute(&pool)
            .await?;
        Ok(r.rows_affected())
    });
    if let Err(e) = res {
        log::error!("work_queue::delete_pending_by_category_blocking failed: {e}");
    }
}

/// Reschedule a claimed job to an explicit moment, blocking. See [`reschedule`].
pub fn reschedule_blocking(db: &Db, id: Uuid, run_after: &str) {
    let pool = db.pool();
    let run_after = run_after.to_string();
    let res = db.block(async move { reschedule(&pool, id, &run_after).await });
    if let Err(e) = res {
        log::error!("work_queue::reschedule_blocking failed: {e}");
    }
}

/// Claim a batch (blocking). Returns empty on error (logged), like the old `claim_next` returning None.
pub fn claim_blocking(db: &Db, category: &str, kind: &str, limit: i64) -> Vec<Job> {
    let pool = db.pool();
    let category = category.to_string();
    let kind = kind.to_string();
    db.block(async move { claim(&pool, &category, &kind, limit).await })
        .unwrap_or_else(|e| {
            log::error!("work_queue::claim_blocking failed: {e}");
            Vec::new()
        })
}

// ---- workflow events (migrated from event_queue) ---------------------------
// A workflow trigger is a single job: the action is always "fire the tenant's OnEvent handlers".
// The event name and emitter go in the payload as metadata, kept separate from `fields` so the
// consumer can hand only the fields to the port-value mapper.

/// Category for inbound workflow-trigger events.
pub const EVENT_CATEGORY: &str = "workflow_event";
/// Kind for workflow-trigger events — one action, so a single kind.
pub const EVENT_KIND: &str = "fire";

/// Enqueue a workflow-trigger event. `event` is the name (`motion`, `telegram_message`, …),
/// `source` the emitter label (`edge`, `telegram`); `fields` is the typed bag the `OnEvent` node
/// exposes. Replaces `event_queue::enqueue`, durably, on the unified queue.
/// Should a workflow event be persisted? `None` = the listener set could not be determined, so
/// deliver (fail open); `Some(set)` is authoritative and an EMPTY set legitimately means nobody
/// listens. Pulled out of [`enqueue_event_blocking`] so the fail-open/no-listener distinction —
/// the part that is easy to collapse and would silently disable the guard — is directly testable.
pub fn should_enqueue_event(
    listened: Option<&std::collections::HashSet<String>>,
    event: &str,
) -> bool {
    match listened {
        None => true,
        Some(set) => set.contains(event),
    }
}

pub fn enqueue_event_blocking(
    db: &Db,
    tenant_id: Uuid,
    event: &str,
    source: &str,
    fields: &serde_json::Value,
) {
    // Nothing enabled handles this event → the job could only ever be consumed and discarded.
    // `fields` can carry the triggering FRAME, so persisting it would cost a multi-MB row for a
    // run that cannot happen. Checked here, at the single choke point every producer goes through,
    // rather than in each caller. `listened_events` fails OPEN, so a lookup error still delivers.
    // `None` = lookup failed → deliver. `Some(set)` is authoritative, INCLUDING an empty set:
    // "this tenant listens for nothing" is the exact case this guard exists for.
    let listened = crate::workflows::repo::listened_events(db, tenant_id);
    if !should_enqueue_event(listened.as_ref(), event) {
        log::debug!("event '{event}' (tenant {tenant_id}) has no enabled listener — not enqueued");
        return;
    }
    let payload = serde_json::json!({ "event": event, "source": source, "fields": fields });
    enqueue_one_blocking(db, tenant_id, EVENT_CATEGORY, EVENT_KIND, payload);
}

// ---- workflow timing windows (DelayTrigger's trailing edge) -----------------
// A debounce window's close is a scheduled job, not a held thread: `run_after` is the window's
// expiry, so it survives a pod restart and fires late rather than never.

/// Category for a workflow node's timing window.
pub const WINDOW_CATEGORY: &str = "workflow_window";
/// Kind for the trailing edge — close the window and let the node fire its `stop`.
pub const WINDOW_KIND: &str = "stop";

/// Schedule a `DelayTrigger`'s trailing stop for `expires_at` (RFC3339). The consumer re-reads
/// the node's state when it comes due, so an extended window reschedules instead of firing.
pub fn enqueue_window_stop_blocking(
    db: &Db,
    tenant_id: Uuid,
    graph_id: Uuid,
    node_id: u32,
    // The instance whose window this closes ("" = the single instance). It travels in the payload
    // because the trailing-edge run can't re-derive it: that run is fired by a timer and carries
    // none of the event's data, so there's no camera to key on. The job is the only thing that
    // still knows which instance armed it.
    instance_key: &str,
    expires_at: &str,
) {
    let payload = serde_json::json!({
        "graph": graph_id, "node": node_id, "instance": instance_key, "expires_at": expires_at,
    });
    enqueue_at_blocking(
        db,
        tenant_id,
        WINDOW_CATEGORY,
        WINDOW_KIND,
        payload,
        expires_at,
    );
}

// ---- monitoring (master: system-wide async load) ----------------------------
// One place to see every async job. All categories, all tenants (or one), so the operator can
// read the system's load — what's queued, running, done, failing — at a glance.

/// A count of jobs in one (category, status). The shape the load panel aggregates.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct CategoryStat {
    pub category: String,
    pub status: String,
    pub count: i64,
}

/// A job row for the load panel. Payload is omitted — this is a volume view, not per-job detail.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct MonitorJob {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub category: String,
    pub kind: String,
    pub status: String,
    pub attempts: i32,
    pub created_at: String,
    pub updated_at: String,
    pub last_error: Option<String>,
}

/// Job counts by (category, status) within the last `window_secs`, across all tenants or one.
/// Scoped to a recent window on purpose — the load view cares about the recent past and live work,
/// not all-time history. `created_at` is cast to `timestamptz` so the window is offset-correct.
pub fn monitor_stats_blocking(
    db: &Db,
    tenant: Option<Uuid>,
    window_secs: i64,
) -> Vec<CategoryStat> {
    let pool = db.pool();
    db.block(async move {
        sqlx::query_as::<_, CategoryStat>(
            "SELECT category, status, COUNT(*) AS count FROM work_jobs
             WHERE ($1::uuid IS NULL OR tenant_id = $1)
               AND created_at::timestamptz >= now() - make_interval(secs => $2::double precision)
             GROUP BY category, status ORDER BY category, status",
        )
        .bind(tenant)
        .bind(window_secs)
        .fetch_all(&pool)
        .await
        .unwrap_or_default()
    })
}

/// A time series of jobs created per `bucket_secs` bucket within the window — `(bucket_start_epoch,
/// count)`, oldest first. Only non-empty buckets; the caller zero-fills the axis. Buckets are
/// floored on the unix epoch so any bucket size (a minute, or coarser for a wide window) lines up.
pub fn monitor_series_blocking(
    db: &Db,
    tenant: Option<Uuid>,
    window_secs: i64,
    bucket_secs: i64,
) -> Vec<(i64, i64)> {
    let pool = db.pool();
    db.block(async move {
        sqlx::query_as::<_, (i64, i64)>(
            "SELECT (floor(extract(epoch FROM created_at::timestamptz) / $3) * $3)::bigint AS t,
                    count(*)::bigint AS c
             FROM work_jobs
             WHERE ($1::uuid IS NULL OR tenant_id = $1)
               AND created_at::timestamptz >= now() - make_interval(secs => $2::double precision)
             GROUP BY t ORDER BY t",
        )
        .bind(tenant)
        .bind(window_secs)
        .bind(bucket_secs)
        .fetch_all(&pool)
        .await
        .unwrap_or_default()
    })
}

/// Recent jobs (newest first) within the last `window_secs`, filterable by tenant/category/status.
#[allow(clippy::too_many_arguments)]
pub fn monitor_list_blocking(
    db: &Db,
    tenant: Option<Uuid>,
    category: Option<&str>,
    status: Option<&str>,
    window_secs: i64,
    limit: i64,
    offset: i64,
) -> (i64, Vec<MonitorJob>) {
    const FILTER: &str = "WHERE ($1::uuid IS NULL OR tenant_id = $1)
          AND ($2::text IS NULL OR category = $2)
          AND ($3::text IS NULL OR status = $3)
          AND created_at::timestamptz >= now() - make_interval(secs => $4::double precision)";
    let pool = db.pool();
    let category = category.filter(|s| !s.is_empty()).map(str::to_string);
    let status = status.filter(|s| !s.is_empty()).map(str::to_string);
    db.block(async move {
        let total: i64 = sqlx::query_scalar(&format!("SELECT count(*) FROM work_jobs {FILTER}"))
            .bind(tenant).bind(category.clone()).bind(status.clone()).bind(window_secs)
            .fetch_one(&pool).await.unwrap_or(0);
        let items = sqlx::query_as::<_, MonitorJob>(&format!(
            "SELECT id, tenant_id, category, kind, status, attempts, created_at, updated_at, last_error
             FROM work_jobs {FILTER} ORDER BY created_at DESC LIMIT $5 OFFSET $6"
        ))
        .bind(tenant).bind(category).bind(status).bind(window_secs).bind(limit).bind(offset)
        .fetch_all(&pool).await.unwrap_or_default();
        (total, items)
    })
}

// ---- push receipts (card #190) ----------------------------------------------
// Expo answers a send in TWO stages. The POST returns a TICKET ("queued"), which is all the send
// path ever read; the RECEIPT, fetched seconds later, carries APNs/FCM's verdict and its reason.
// On 2026-08-17 every Device Gate push was dying at Apple with BadEnvironmentKeyInToken while the
// run log read `asked iPhone` -> `expired`, which is indistinguishable from the person ignoring it.
//
// The check cannot run inline: the sender must not block, and the receipt is not ready yet. So it
// is a delayed job, and the queue's `run_after` is exactly the mechanism for that.

/// Category for deferred push-receipt checks.
pub const PUSH_RECEIPT_CATEGORY: &str = "push_receipt";
/// Kind — one action.
pub const PUSH_RECEIPT_KIND: &str = "check";

// ---- notification sends (migrated from notification_queue) ------------------
// One send per job. The send is non-idempotent (a re-run is a duplicate Telegram), so it relies
// on the atomic claim; transient failures reschedule with backoff via `retry_after`.

/// Category for outbound notification sends.
pub const NOTIFICATION_CATEGORY: &str = "notification";
/// Kind for a send — one action, so a single kind.
pub const NOTIFICATION_KIND: &str = "send";

/// Enqueue a notification send. `target` = beacon label (or `None` for all recipients);
/// `chat_id` = a direct Telegram chat reply (or `None`); `image_key`/`video_key` = object-store
/// keys. Replaces `notification::queue::enqueue`, on the unified queue.
#[allow(clippy::too_many_arguments)]
pub fn enqueue_notification_blocking(
    db: &Db,
    tenant_id: Uuid,
    target: Option<&str>,
    chat_id: Option<i64>,
    message: &str,
    image_key: Option<&str>,
    video_key: Option<&str>,
) {
    let payload = serde_json::json!({
        // Delivery channel (card #139). Absent/"telegram" = today's path; the consumer defaults to
        // telegram so every pre-existing queued job keeps working unchanged.
        "channel": "telegram",
        "target": target,
        "chat_id": chat_id,
        "message": message,
        "image_key": image_key,
        "video_key": video_key,
    });
    enqueue_one_blocking(
        db,
        tenant_id,
        NOTIFICATION_CATEGORY,
        NOTIFICATION_KIND,
        payload,
    );
}

/// Queue a **mobile push** send (card #139) — same durable queue and retry/backoff as Telegram,
/// distinguished only by `channel`. `target` optionally addresses one registered device by label;
/// `None` fans out to every enabled device for the tenant.
pub fn enqueue_push_notification_blocking(
    db: &Db,
    tenant_id: Uuid,
    target: Option<&str>,
    title: &str,
    message: &str,
) {
    let payload = serde_json::json!({
        "channel": "push",
        "target": target,
        "title": title,
        "message": message,
    });
    enqueue_one_blocking(
        db,
        tenant_id,
        NOTIFICATION_CATEGORY,
        NOTIFICATION_KIND,
        payload,
    );
}

/// Queue a **transactional email** (card #165) — the account-level messages a self-serve product
/// needs: password reset, address confirmation, receipts, trial expiry.
///
/// Same durable queue, retry and backoff as Telegram and push, distinguished only by `channel`.
/// That is the point of putting email here rather than in its own path: a provider outage becomes
/// a retry instead of a lost password reset, and every send is visible in the same queue view.
///
/// `to` is an email address, not a beacon label — these messages go to ONE person about THEIR
/// account and must work for a user with no tenant configuration at all.
pub fn enqueue_email_blocking(
    db: &Db,
    tenant_id: Uuid,
    to: &str,
    subject: &str,
    body: &str,
    html: Option<&str>,
) {
    let payload = serde_json::json!({
        "channel": "email",
        "to": to,
        "subject": subject,
        "message": body,
        "html": html,
    });
    enqueue_one_blocking(
        db,
        tenant_id,
        NOTIFICATION_CATEGORY,
        NOTIFICATION_KIND,
        payload,
    );
}

/// A terminal notification job whose staged media can be reclaimed.
pub struct PurgeableMedia {
    pub id: Uuid,
    pub image_key: Option<String>,
    pub video_key: Option<String>,
}

/// Terminal notification sends past the grace window whose media keys are **not** still referenced
/// by an in-flight send (one staged object may back several jobs). The object store can then drop
/// the objects. Mirrors the old `notification::queue::purgeable_media`, reading keys from payload.
pub fn purgeable_notification_media_blocking(
    db: &Db,
    grace_secs: i64,
    limit: i64,
) -> Vec<PurgeableMedia> {
    let pool = db.pool();
    db.block(async move {
        sqlx::query_as::<_, (Uuid, Option<String>, Option<String>)>(
            "SELECT q.id, q.payload->>'image_key', q.payload->>'video_key'
             FROM work_jobs q
             WHERE q.category = 'notification' AND q.status IN ('done', 'failed')
               AND (q.payload->>'image_key' IS NOT NULL OR q.payload->>'video_key' IS NOT NULL)
               AND q.updated_at::timestamptz < now() - make_interval(secs => $1)
               AND NOT EXISTS (
                   SELECT 1 FROM work_jobs o
                   WHERE o.category = 'notification' AND o.status NOT IN ('done', 'failed')
                     AND ((q.payload->>'image_key' IS NOT NULL AND o.payload->>'image_key' = q.payload->>'image_key')
                       OR (q.payload->>'video_key' IS NOT NULL AND o.payload->>'video_key' = q.payload->>'video_key'))
               )
             ORDER BY q.updated_at
             LIMIT $2",
        )
        .bind(grace_secs).bind(limit)
        .fetch_all(&pool).await
        .unwrap_or_default()
        .into_iter().map(|(id, image_key, video_key)| PurgeableMedia { id, image_key, video_key }).collect()
    })
}

/// Strip the media keys from a job's payload once the objects are gone, so nothing points at a
/// deleted attachment. Mirrors `notification::queue::clear_media_keys`.
pub fn clear_notification_media_keys_blocking(db: &Db, ids: &[Uuid]) -> u64 {
    let pool = db.pool();
    let ids = ids.to_vec();
    db.block(async move {
        sqlx::query("UPDATE work_jobs SET payload = (payload - 'image_key') - 'video_key' WHERE id = ANY($1)")
            .bind(&ids)
            .execute(&pool).await
            .map(|r| r.rows_affected()).unwrap_or(0)
    })
}

/// Requeue with a backoff delay (blocking) — the send hit a transient error and has retries left.
pub fn retry_after_blocking(db: &Db, id: Uuid, backoff_secs: i64, err: &str) {
    let pool = db.pool();
    let err = err.to_string();
    if let Err(e) = db.block(async move { retry_after(&pool, id, backoff_secs, &err).await }) {
        log::error!("work_queue::retry_after_blocking failed: {e}");
    }
}

/// Mark done (blocking).
pub fn mark_done_blocking(db: &Db, id: Uuid) {
    let pool = db.pool();
    if let Err(e) = db.block(async move { mark_done(&pool, id).await }) {
        log::error!("work_queue::mark_done_blocking failed: {e}");
    }
}

/// Mark failed (blocking).
pub fn mark_failed_blocking(db: &Db, id: Uuid, err: &str, max_attempts: i32) {
    let pool = db.pool();
    let err = err.to_string();
    if let Err(e) = db.block(async move { mark_failed(&pool, id, &err, max_attempts).await }) {
        log::error!("work_queue::mark_failed_blocking failed: {e}");
    }
}

#[cfg(test)]
mod event_enqueue_guard_tests {
    use super::should_enqueue_event;
    use std::collections::HashSet;

    /// The production case: a tenant with every workflow disabled listens for nothing, and its
    /// motion pushes (which carry the frame inline) must not be persisted. 39,110 such jobs had
    /// accumulated to 6.5 GB before this guard existed.
    #[test]
    fn an_empty_listener_set_skips_the_event() {
        let none_listening = HashSet::new();
        assert!(!should_enqueue_event(Some(&none_listening), "motion"));
        assert!(!should_enqueue_event(
            Some(&none_listening),
            "face_detected"
        ));
    }

    /// An unknown listener set (query failed) must still deliver — dropping events because the
    /// database hiccuped would be a far worse failure than the write amplification it prevents.
    #[test]
    fn an_unknown_listener_set_delivers() {
        assert!(should_enqueue_event(None, "motion"));
    }

    /// Only the events actually handled get through.
    #[test]
    fn only_listened_events_are_enqueued() {
        let listening = HashSet::from(["motion".to_string()]);
        assert!(should_enqueue_event(Some(&listening), "motion"));
        assert!(!should_enqueue_event(Some(&listening), "face_detected"));
    }
}
