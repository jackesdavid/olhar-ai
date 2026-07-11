//! Generic durable work queue (card #64). One table, `work_jobs`, for asynchronous jobs across
//! the system: enqueue, claim, retry, and monitor from a single place. Jobs are grouped by
//! `category` (the domain) and `kind` (the action), keyed to a `request_id` so the several jobs
//! of one request can be tracked together.
//!
//! Async-first: consumers wake on `LISTEN` and the producer `NOTIFY`s on enqueue, so latency is
//! low without a tight poll. Consumers must be **idempotent** — a job may be delivered more than
//! once (a NOTIFY plus a poll, or two replicas), and re-doing a done action must be a no-op.

use serde::Serialize;
use sqlx::PgPool;
use uuid::Uuid;

use crate::alerts::now_rfc3339;

/// The `NOTIFY`/`LISTEN` channel every consumer watches. Payload is the job's `category`.
pub const CHANNEL: &str = "work_jobs";

/// A claimed job. `payload` is self-contained — the consumer needs nothing else to act.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Job {
    pub id:         Uuid,
    pub request_id: Uuid,
    pub tenant_id:  Uuid,
    pub category:   String,
    pub kind:       String,
    pub payload:    serde_json::Value,
    pub attempts:   i32,
}

/// One job to enqueue: its action (`kind`) and self-contained data.
pub struct NewJob {
    pub kind:    String,
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
        sqlx::query(
            "INSERT INTO work_jobs (request_id, tenant_id, category, kind, payload, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $6)",
        )
        .bind(request_id).bind(tenant_id).bind(category).bind(&j.kind).bind(&j.payload).bind(&now)
        .execute(&mut *tx).await?;
    }
    // NOTIFY inside the tx so it fires exactly when the rows commit (Postgres holds notifications
    // until commit and drops them on rollback).
    sqlx::query("SELECT pg_notify($1, $2)").bind(CHANNEL).bind(category).execute(&mut *tx).await?;
    tx.commit().await
}

/// Claim up to `limit` open jobs of a (category, kind), oldest first. Not row-locked — consumers
/// are idempotent, so at worst a job is processed twice, which is harmless and simpler than
/// holding locks across slow I/O (an R2 delete).
pub async fn claim(pool: &PgPool, category: &str, kind: &str, limit: i64) -> sqlx::Result<Vec<Job>> {
    sqlx::query_as::<_, Job>(
        "SELECT id, request_id, tenant_id, category, kind, payload, attempts
         FROM work_jobs
         WHERE category = $1 AND kind = $2 AND status = 'pending'
         ORDER BY created_at LIMIT $3",
    )
    .bind(category).bind(kind).bind(limit)
    .fetch_all(pool).await
}

/// Mark a job done. Idempotent on the row.
pub async fn mark_done(pool: &PgPool, id: Uuid) -> sqlx::Result<()> {
    sqlx::query("UPDATE work_jobs SET status = 'done', updated_at = $2 WHERE id = $1")
        .bind(id).bind(now_rfc3339())
        .execute(pool).await.map(|_| ())
}

/// Record a failure: bump attempts, keep it `pending` for retry until `max_attempts`, then park it
/// `failed` so a poisoned job can't spin forever.
pub async fn mark_failed(pool: &PgPool, id: Uuid, err: &str, max_attempts: i32) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE work_jobs
         SET attempts = attempts + 1,
             last_error = $2,
             status = CASE WHEN attempts + 1 >= $3 THEN 'failed' ELSE 'pending' END,
             updated_at = $4
         WHERE id = $1",
    )
    .bind(id).bind(err).bind(max_attempts).bind(now_rfc3339())
    .execute(pool).await.map(|_| ())
}

/// Summary of a request's jobs by status — the "did my deletion finish?" query. Consumers are
/// independent, so a request is complete when nothing is left `pending`; anything `failed` is a
/// leg that needs attention (surfaced in monitoring), not a blocker on the others.
pub async fn request_status(pool: &PgPool, request_id: Uuid) -> sqlx::Result<Vec<(String, String, i64)>> {
    sqlx::query_as::<_, (String, String, i64)>(
        "SELECT kind, status, COUNT(*) FROM work_jobs
         WHERE request_id = $1 GROUP BY kind, status",
    )
    .bind(request_id)
    .fetch_all(pool).await
}

/// The `recording_id`s a tenant has an open (non-done) job for — the list marks these clips as
/// deletion-in-progress. Reads the JSONB payload, so it works for any category that stamps one.
pub async fn open_recording_ids(pool: &PgPool, tenant_id: Uuid) -> sqlx::Result<Vec<i64>> {
    let ids: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT payload->>'recording_id' FROM work_jobs
         WHERE tenant_id = $1 AND status <> 'done' AND payload ? 'recording_id'",
    )
    .bind(tenant_id)
    .fetch_all(pool).await?;
    Ok(ids.into_iter().filter_map(|s| s.parse::<i64>().ok()).collect())
}
