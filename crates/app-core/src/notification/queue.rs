//! Durable notification queue — persistence for the notification service's send jobs.
//!
//! Producers (flow's `RemoteNotifier`) [`enqueue`]; `sentinel-notification` [`claim_next`]s a
//! due row (concurrency-safe via `FOR UPDATE SKIP LOCKED`), sends, then marks it terminal
//! ([`mark_sent`]) or requeues with backoff ([`mark_retry_or_fail`]). All read-only observability
//! lives elsewhere and never touches this claim path.
use uuid::Uuid;

use crate::alerts::now_rfc3339;
use crate::db::Db;

/// A claimed send job (status already flipped to `sending`).
#[derive(Debug, Clone)]
pub struct QueuedNotification {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub attempts: i32,
    /// Beacon label to address, or `None` for all enabled recipients (ignored when `chat_id` is set).
    pub target: Option<String>,
    /// A raw Telegram chat to reply to directly (card #15). `Some` = send straight to this chat,
    /// bypassing beacon resolution; `None` = use `target`/all-recipients.
    pub chat_id: Option<i64>,
    pub message: String,
    pub image_key: Option<String>,
    pub video_key: Option<String>,
}

/// Enqueue a notification for `tenant_id`. `target` = a beacon label, or `None` for every enabled
/// recipient. `chat_id` = `Some` to reply direct to a specific Telegram chat (bot reply, card #15),
/// bypassing beacon resolution. `image_key`/`video_key` are object-store keys. Best-effort; returns
/// the new row id.
#[allow(clippy::too_many_arguments)]
pub fn enqueue(
    db: &Db,
    tenant_id: Uuid,
    target: Option<&str>,
    chat_id: Option<i64>,
    message: &str,
    image_key: Option<&str>,
    video_key: Option<&str>,
) -> Uuid {
    let id = Uuid::new_v4();
    let now = now_rfc3339();
    let pool = db.pool();
    let target = target.map(str::to_string);
    let message = message.to_string();
    let image_key = image_key.map(str::to_string);
    let video_key = video_key.map(str::to_string);
    let res: Result<_, sqlx::Error> = db.block(async move {
        sqlx::query(
            "INSERT INTO notification_queue
                (id, tenant_id, status, attempts, target, chat_id, message, image_key, video_key, created_at, next_attempt_at)
             VALUES ($1, $2, 'queued', 0, $3, $4, $5, $6, $7, $8, $8)",
        )
        .bind(id)
        .bind(tenant_id)
        .bind(target)
        .bind(chat_id)
        .bind(message)
        .bind(image_key)
        .bind(video_key)
        .bind(&now)
        .execute(&pool)
        .await
    });
    if let Err(e) = res {
        log::error!("notification::queue::enqueue failed: {e}");
    }
    id
}

/// Claim the next due job, flipping it to `sending` and bumping `attempts`. Returns `None`
/// when nothing is due. Safe across multiple consumers (`FOR UPDATE SKIP LOCKED`).
pub fn claim_next(db: &Db) -> Option<QueuedNotification> {
    let now = now_rfc3339();
    let pool = db.pool();
    db.block(async move {
        let mut tx = pool.begin().await.ok()?;
        let row: (Uuid, Uuid, i32, Option<String>, Option<i64>, String, Option<String>, Option<String>) =
            sqlx::query_as(
                "SELECT id, tenant_id, attempts, target, chat_id, message, image_key, video_key
                 FROM notification_queue
                 WHERE status = 'queued' AND next_attempt_at <= $1
                 ORDER BY next_attempt_at
                 LIMIT 1
                 FOR UPDATE SKIP LOCKED",
            )
            .bind(&now)
            .fetch_optional(&mut *tx)
            .await
            .ok()??;
        sqlx::query(
            "UPDATE notification_queue SET status = 'sending', attempts = attempts + 1 WHERE id = $1",
        )
        .bind(row.0)
        .execute(&mut *tx)
        .await
        .ok()?;
        tx.commit().await.ok()?;
        Some(QueuedNotification {
            id: row.0,
            tenant_id: row.1,
            attempts: row.2 + 1,
            target: row.3,
            chat_id: row.4,
            message: row.5,
            image_key: row.6,
            video_key: row.7,
        })
    })
}

/// Mark a job delivered.
pub fn mark_sent(db: &Db, id: Uuid, result: &str) {
    let now = now_rfc3339();
    let pool = db.pool();
    let result = result.to_string();
    let res: Result<_, sqlx::Error> = db.block(async move {
        sqlx::query("UPDATE notification_queue SET status = 'sent', sent_at = $2, result = $3 WHERE id = $1")
            .bind(id)
            .bind(&now)
            .bind(&result)
            .execute(&pool)
            .await
    });
    if let Err(e) = res {
        log::error!("notification::queue::mark_sent failed: {e}");
    }
}

/// Requeue with a backoff delay, or mark `failed` once `attempts >= max_attempts`.
pub fn mark_retry_or_fail(
    db: &Db,
    id: Uuid,
    attempts: i32,
    max_attempts: i32,
    backoff_secs: i64,
    error: &str,
) {
    let pool = db.pool();
    let error = error.to_string();
    let terminal = attempts >= max_attempts;
    let next = (chrono::Local::now() + chrono::Duration::seconds(backoff_secs)).to_rfc3339();
    let res: Result<_, sqlx::Error> = db.block(async move {
        if terminal {
            sqlx::query("UPDATE notification_queue SET status = 'failed', result = $2 WHERE id = $1")
                .bind(id)
                .bind(&error)
                .execute(&pool)
                .await
        } else {
            sqlx::query(
                "UPDATE notification_queue SET status = 'queued', next_attempt_at = $2, result = $3 WHERE id = $1",
            )
            .bind(id)
            .bind(&next)
            .bind(&error)
            .execute(&pool)
            .await
        }
    });
    if let Err(e) = res {
        log::error!("notification::queue::mark_retry_or_fail failed: {e}");
    }
}

// ---- observability (read-only, tenant-scoped) -------------------------------
// Plain SELECT aggregates — no locks, never touches the claim/send path.

/// Local start-of-day as an RFC3339 string (same format the rows use, so text comparison works).
fn today_start_rfc3339() -> String {
    chrono::Local::now()
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .and_then(|dt| dt.and_local_timezone(chrono::Local).single())
        .map(|dt| dt.to_rfc3339())
        .unwrap_or_default()
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct HourBucket {
    pub hour: String, // "YYYY-MM-DDTHH"
    pub count: i64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct QueueStats {
    pub pending: i64,      // queued + sending
    pub sent_today: i64,
    pub failed_today: i64,
    pub queued_today: i64, // total enqueued today
    pub hourly: Vec<HourBucket>, // today, per hour (enqueued)
}

/// Tenant-scoped queue metrics for the observability dashboard.
pub fn stats(db: &Db, tenant_id: uuid::Uuid) -> QueueStats {
    let today = today_start_rfc3339();
    let pool = db.pool();
    db.block(async move {
        let scalar = |sql: &'static str, extra_today: bool| {
            let pool = pool.clone();
            let today = today.clone();
            async move {
                let mut q = sqlx::query_scalar::<_, i64>(sql).bind(tenant_id);
                if extra_today {
                    q = q.bind(today);
                }
                q.fetch_one(&pool).await.unwrap_or(0)
            }
        };
        let pending = scalar(
            "SELECT count(*) FROM notification_queue WHERE tenant_id = $1 AND status IN ('queued','sending')",
            false,
        )
        .await;
        let sent_today = scalar(
            "SELECT count(*) FROM notification_queue WHERE tenant_id = $1 AND status = 'sent' AND created_at >= $2",
            true,
        )
        .await;
        let failed_today = scalar(
            "SELECT count(*) FROM notification_queue WHERE tenant_id = $1 AND status = 'failed' AND created_at >= $2",
            true,
        )
        .await;
        let queued_today = scalar(
            "SELECT count(*) FROM notification_queue WHERE tenant_id = $1 AND created_at >= $2",
            true,
        )
        .await;
        let hourly = sqlx::query_as::<_, (String, i64)>(
            "SELECT left(created_at, 13) AS hr, count(*) FROM notification_queue
             WHERE tenant_id = $1 AND created_at >= $2 GROUP BY hr ORDER BY hr",
        )
        .bind(tenant_id)
        .bind(&today)
        .fetch_all(&pool)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|(hour, count)| HourBucket { hour, count })
        .collect();
        QueueStats { pending, sent_today, failed_today, queued_today, hourly }
    })
}

#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
pub struct QueueItem {
    pub id: uuid::Uuid,
    pub status: String,
    pub target: Option<String>,
    pub message: String,
    pub attempts: i32,
    pub image_key: Option<String>,
    pub video_key: Option<String>,
    pub result: Option<String>,
    pub created_at: String,
    pub sent_at: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct QueuePage {
    pub total: i64,
    pub items: Vec<QueueItem>,
}

/// Tenant-scoped, paginated, searchable queue rows (newest first). `status` filters exactly;
/// `q` matches message/target case-insensitively. Fixed SQL with nullable-param filters —
/// `$n IS NULL OR …` — so no dynamic string building.
pub fn list(
    db: &Db,
    tenant_id: uuid::Uuid,
    status: Option<&str>,
    q: Option<&str>,
    limit: i64,
    offset: i64,
) -> QueuePage {
    const FILTER: &str = "WHERE tenant_id = $1
          AND ($2::text IS NULL OR status = $2)
          AND ($3::text IS NULL OR lower(message) LIKE $3 OR lower(coalesce(target,'')) LIKE $3)";
    let pool = db.pool();
    let status = status.filter(|s| !s.is_empty()).map(str::to_string);
    let pattern = q
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .map(|s| format!("%{s}%"));
    db.block(async move {
        let total: i64 = sqlx::query_scalar(&format!("SELECT count(*) FROM notification_queue {FILTER}"))
            .bind(tenant_id)
            .bind(status.clone())
            .bind(pattern.clone())
            .fetch_one(&pool)
            .await
            .unwrap_or(0);
        let items = sqlx::query_as::<_, QueueItem>(&format!(
            "SELECT id, status, target, message, attempts, image_key, video_key, result, created_at, sent_at
             FROM notification_queue {FILTER}
             ORDER BY created_at DESC LIMIT $4 OFFSET $5"
        ))
        .bind(tenant_id)
        .bind(status)
        .bind(pattern)
        .bind(limit)
        .bind(offset)
        .fetch_all(&pool)
        .await
        .unwrap_or_default();
        QueuePage { total, items }
    })
}
