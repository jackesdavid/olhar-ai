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
    /// Beacon label to address, or `None` for all enabled recipients.
    pub target: Option<String>,
    pub message: String,
    pub image_key: Option<String>,
    pub video_key: Option<String>,
}

/// Enqueue a notification for `tenant_id`. `target` = a beacon label, or `None` for every
/// enabled recipient. `image_key`/`video_key` are object-store keys (media plumbing added later).
/// Best-effort; returns the new row id.
pub fn enqueue(
    db: &Db,
    tenant_id: Uuid,
    target: Option<&str>,
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
                (id, tenant_id, status, attempts, target, message, image_key, video_key, created_at, next_attempt_at)
             VALUES ($1, $2, 'queued', 0, $3, $4, $5, $6, $7, $7)",
        )
        .bind(id)
        .bind(tenant_id)
        .bind(target)
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
        let row: (Uuid, Uuid, i32, Option<String>, String, Option<String>, Option<String>) =
            sqlx::query_as(
                "SELECT id, tenant_id, attempts, target, message, image_key, video_key
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
            message: row.4,
            image_key: row.5,
            video_key: row.6,
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
