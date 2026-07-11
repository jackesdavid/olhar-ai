//! Durable inbound event queue — the symmetric twin of [`crate::notification::queue`].
//!
//! Producers (control's edge relays: `motion`, `edge:full-disk`; the notification service's
//! `telegram_message`) [`enqueue`] a trigger event; `sentinel-flow` [`claim_next`]s the oldest
//! queued row (concurrency-safe via `FOR UPDATE SKIP LOCKED`), fires it into the engine as an
//! `OnEvent` trigger, then marks it terminal ([`mark_done`] / [`mark_failed`]).
//!
//! This retires the lossy Redis `flow:inject` pub/sub: an event survives a flow restart and is
//! never dropped when the fabric is down. Payload is JSON (bound/read as text so we don't need
//! sqlx's `json` feature — the column is `jsonb`, cast at the query boundary).
use uuid::Uuid;

use crate::timeutil::now_rfc3339;
use crate::db::Db;

/// A claimed inbound event (status already flipped to `claimed`).
#[derive(Debug, Clone)]
pub struct ClaimedEvent {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub event: String,
    pub source: String,
    pub attempts: i32,
    pub payload: serde_json::Value,
}

/// Enqueue a trigger event for `tenant_id`. `source` is the emitter label (e.g. `"edge"`,
/// `"telegram"`); `payload` is the typed field bag the `OnEvent` node will expose. Best-effort;
/// returns the new row id (durable — survives a consumer restart, unlike the old `flow:inject`).
pub fn enqueue(
    db: &Db,
    tenant_id: Uuid,
    event: &str,
    source: &str,
    payload: &serde_json::Value,
) -> Uuid {
    let id = Uuid::new_v4();
    let now = now_rfc3339();
    let pool = db.pool();
    let event = event.to_string();
    let source = source.to_string();
    let payload = payload.to_string();
    let res: Result<_, sqlx::Error> = db.block(async move {
        sqlx::query(
            "INSERT INTO event_queue
                (id, tenant_id, event, source, payload, status, attempts, created_at)
             VALUES ($1, $2, $3, $4, $5::jsonb, 'queued', 0, $6)",
        )
        .bind(id)
        .bind(tenant_id)
        .bind(&event)
        .bind(&source)
        .bind(&payload)
        .bind(&now)
        .execute(&pool)
        .await
    });
    if let Err(e) = res {
        log::error!("event_queue::enqueue failed: {e}");
    }
    id
}

/// Claim the oldest queued event, flipping it to `claimed` and bumping `attempts`. Returns
/// `None` when nothing is pending. Safe across multiple flow pods (`FOR UPDATE SKIP LOCKED`).
pub fn claim_next(db: &Db) -> Option<ClaimedEvent> {
    let now = now_rfc3339();
    let pool = db.pool();
    db.block(async move {
        let mut tx = pool.begin().await.ok()?;
        let row: (Uuid, Uuid, String, String, i32, String) = sqlx::query_as(
            "SELECT id, tenant_id, event, source, attempts, payload::text
             FROM event_queue
             WHERE status = 'queued'
             ORDER BY created_at
             LIMIT 1
             FOR UPDATE SKIP LOCKED",
        )
        .fetch_optional(&mut *tx)
        .await
        .ok()??;
        sqlx::query(
            "UPDATE event_queue SET status = 'claimed', attempts = attempts + 1, claimed_at = $2 WHERE id = $1",
        )
        .bind(row.0)
        .bind(&now)
        .execute(&mut *tx)
        .await
        .ok()?;
        tx.commit().await.ok()?;
        let payload = serde_json::from_str(&row.5).unwrap_or(serde_json::Value::Null);
        Some(ClaimedEvent {
            id: row.0,
            tenant_id: row.1,
            event: row.2,
            source: row.3,
            attempts: row.4 + 1,
            payload,
        })
    })
}

/// Mark an event handled (it fired into the engine).
pub fn mark_done(db: &Db, id: Uuid) {
    let pool = db.pool();
    let res: Result<_, sqlx::Error> = db.block(async move {
        sqlx::query("UPDATE event_queue SET status = 'done' WHERE id = $1")
            .bind(id)
            .execute(&pool)
            .await
    });
    if let Err(e) = res {
        log::error!("event_queue::mark_done failed: {e}");
    }
}

/// Mark an event `failed` (unprocessable payload). Events fire-and-forget into the engine, so
/// this is only for malformed rows — there's no send to retry, so no backoff.
pub fn mark_failed(db: &Db, id: Uuid, error: &str) {
    let pool = db.pool();
    let error = error.to_string();
    let res: Result<_, sqlx::Error> = db.block(async move {
        sqlx::query("UPDATE event_queue SET status = 'failed' WHERE id = $1")
            .bind(id)
            .execute(&pool)
            .await
    });
    if let Err(e) = res {
        log::error!("event_queue::mark_failed failed: {e}");
    }
    log::warn!("event_queue {id} failed: {error}");
}
