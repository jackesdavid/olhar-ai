//! Cloud→edge command channel (card #144) — the durable half of the SSE command transport.
//!
//! **Why this exists.** The tailnet let the cloud reach *into* a LAN edge to command it (record,
//! snapshot, probe). An on-device edge (phone/Android) is outbound-only — it can reach the cloud
//! but the cloud cannot reach it — and the plan is to retire the tailnet for *every* edge. So the
//! edge holds one outbound SSE stream to control and commands ride *down* it; this module is the
//! durable queue behind that stream.
//!
//! **Shape.** A caller (the workflow RecordClip node, on any pod/service) [`enqueue`]s a command
//! for a box and NOTIFYs. The control pod that holds that box's SSE stream (and only it) claims the
//! command ([`claim_for_box`]) and pushes it down the wire. The edge runs it locally and POSTs the
//! result, which [`finish`]es the row and NOTIFYs the result channel; the caller
//! [`await_result_blocking`]s on the row. Delivery is box-scoped by construction, so it is
//! multi-pod correct — the connection, not the caller's pod, selects the deliverer.
//!
//! Mirrors [`crate::work_queue`]'s claim/NOTIFY/reaper semantics; distinct table because delivery
//! is per-box (not per (category,kind)) and each command carries a result.

use serde::Serialize;
use sqlx::PgPool;
use uuid::Uuid;

use crate::db::Db;
use crate::timeutil::now_rfc3339;

/// NOTIFY/LISTEN channel raised on enqueue; payload is the target `box_id` so a pod can decide in
/// O(1) whether it holds that box before it bothers to claim.
pub const CHANNEL: &str = "edge_commands";
/// NOTIFY channel raised when a command reaches a terminal state; payload is the command `id`.
pub const RESULT_CHANNEL: &str = "edge_command_result";

/// A `running` command untouched for this long is presumed undelivered (the holding pod died or the
/// edge dropped mid-flight) and re-claimed. Short — commands are latency-sensitive and idempotent
/// enough (a duplicate record start coalesces on the edge) that quick redelivery beats waiting.
const STALE_SECS: i64 = 45;

/// A command claimed for delivery. `args` is self-contained — the edge needs nothing else to act.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Command {
    pub id:        Uuid,
    pub tenant_id: Uuid,
    pub box_id:    Uuid,
    pub cmd:       String,
    pub args:      serde_json::Value,
    pub attempts:  i32,
}

// ---- producer (any pod/service) --------------------------------------------

/// Enqueue a command for a box and NOTIFY so the holding pod delivers it immediately. `ttl_secs`
/// bounds how long an undelivered command stays live — past `expires_at` it is never claimed (the
/// edge was offline the whole time), so the caller's `await` times out instead of the edge running
/// a stale command minutes later. Returns the command id to await on.
pub async fn enqueue(
    pool: &PgPool,
    tenant_id: Uuid,
    box_id: Uuid,
    cmd: &str,
    args: serde_json::Value,
    ttl_secs: i64,
) -> sqlx::Result<Uuid> {
    let id = Uuid::new_v4();
    let now = now_rfc3339();
    let expires = (chrono::Local::now() + chrono::Duration::seconds(ttl_secs)).to_rfc3339();
    let mut tx = pool.begin().await?;
    sqlx::query(
        "INSERT INTO edge_commands (id, tenant_id, box_id, cmd, args, created_at, updated_at, run_after, expires_at)
         VALUES ($1, $2, $3, $4, $5, $6, $6, $6, $7)",
    )
    .bind(id).bind(tenant_id).bind(box_id).bind(cmd).bind(&args).bind(&now).bind(&expires)
    .execute(&mut *tx).await?;
    // NOTIFY inside the tx so it fires exactly on commit (dropped on rollback). Payload = box_id.
    sqlx::query("SELECT pg_notify($1, $2)").bind(CHANNEL).bind(box_id.to_string()).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(id)
}

// ---- deliverer (the control pod holding the box's SSE stream) ---------------

/// Atomically claim up to `limit` deliverable commands for ONE box: `ready` rows that haven't
/// expired, plus stale `running` rows (reaped), locked `FOR UPDATE SKIP LOCKED` and flipped to
/// `running`. Only the pod that holds this box's stream calls it, so a command is delivered once.
pub async fn claim_for_box(pool: &PgPool, box_id: Uuid, limit: i64) -> sqlx::Result<Vec<Command>> {
    let now = now_rfc3339();
    let stale_before = (chrono::Local::now() - chrono::Duration::seconds(STALE_SECS)).to_rfc3339();
    let mut tx = pool.begin().await?;
    let cmds = sqlx::query_as::<_, Command>(
        "SELECT id, tenant_id, box_id, cmd, args, attempts
         FROM edge_commands
         WHERE box_id = $1
           AND expires_at > $2
           AND ( (status = 'ready'   AND run_after <= $2)
              OR (status = 'running' AND updated_at < $3) )
         ORDER BY run_after
         LIMIT $4
         FOR UPDATE SKIP LOCKED",
    )
    .bind(box_id).bind(&now).bind(&stale_before).bind(limit)
    .fetch_all(&mut *tx).await?;

    if !cmds.is_empty() {
        let ids: Vec<Uuid> = cmds.iter().map(|c| c.id).collect();
        sqlx::query(
            "UPDATE edge_commands SET status = 'running', attempts = attempts + 1, updated_at = $2
             WHERE id = ANY($1)",
        )
        .bind(&ids).bind(&now)
        .execute(&mut *tx).await?;
    }
    tx.commit().await?;
    Ok(cmds.into_iter().map(|mut c| { c.attempts += 1; c }).collect())
}

/// Return a claimed command to `ready` (delivery failed — the SSE send errored) so another pass, or
/// the box's reconnect, redelivers it. A short hold avoids a busy-loop.
pub async fn release(pool: &PgPool, id: Uuid, delay_secs: i64) -> sqlx::Result<()> {
    let run_after = (chrono::Local::now() + chrono::Duration::seconds(delay_secs)).to_rfc3339();
    sqlx::query("UPDATE edge_commands SET status = 'ready', run_after = $2, updated_at = $3 WHERE id = $1")
        .bind(id).bind(&run_after).bind(now_rfc3339())
        .execute(pool).await.map(|_| ())
}

/// Record a command's terminal result (from the edge's result POST) and NOTIFY the result channel so
/// the waiting caller wakes immediately. `ok=false` parks it `failed` with `error`.
pub async fn finish(
    pool: &PgPool,
    id: Uuid,
    ok: bool,
    result: Option<serde_json::Value>,
    error: Option<&str>,
) -> sqlx::Result<()> {
    let status = if ok { "done" } else { "failed" };
    let mut tx = pool.begin().await?;
    sqlx::query("UPDATE edge_commands SET status = $2, result = $3, error = $4, updated_at = $5 WHERE id = $1")
        .bind(id).bind(status).bind(&result).bind(error).bind(now_rfc3339())
        .execute(&mut *tx).await?;
    sqlx::query("SELECT pg_notify($1, $2)").bind(RESULT_CHANNEL).bind(id.to_string()).execute(&mut *tx).await?;
    tx.commit().await
}

// ---- session tracking (which boxes are reachable over SSE) ------------------

/// Mark this box's SSE stream live on `pod` — called on connect and on a periodic heartbeat so a
/// caller on any pod can read `sse_last_seen` and route a command over SSE instead of the tailnet.
pub async fn mark_session(pool: &PgPool, box_id: Uuid, pod: &str) -> sqlx::Result<()> {
    sqlx::query("UPDATE tenant_boxes SET sse_last_seen = $2, sse_pod = $3 WHERE id = $1")
        .bind(box_id).bind(now_rfc3339()).bind(pod)
        .execute(pool).await.map(|_| ())
}

/// Clear this box's SSE session on disconnect — but only if it's still ours, so a race where the box
/// reconnected to another pod (which already claimed the session) doesn't wipe the newer session.
pub async fn clear_session(pool: &PgPool, box_id: Uuid, pod: &str) -> sqlx::Result<()> {
    sqlx::query("UPDATE tenant_boxes SET sse_last_seen = NULL, sse_pod = NULL WHERE id = $1 AND sse_pod = $2")
        .bind(box_id).bind(pod)
        .execute(pool).await.map(|_| ())
}

// ---- blocking wrappers (flow: std threads with a `Db`, no PgPool) -----------

/// Enqueue a command, blocking. Returns the id to await, or None on error (logged).
pub fn enqueue_blocking(
    db: &Db,
    tenant_id: Uuid,
    box_id: Uuid,
    cmd: &str,
    args: serde_json::Value,
    ttl_secs: i64,
) -> Option<Uuid> {
    let pool = db.pool();
    let cmd = cmd.to_string();
    db.block(async move { enqueue(&pool, tenant_id, box_id, &cmd, args, ttl_secs).await })
        .map_err(|e| log::error!("edge_commands::enqueue_blocking failed: {e}"))
        .ok()
}

/// Whether the box has a live SSE session (its `sse_last_seen` is within `max_age_secs`). Drives the
/// caller's SSE-vs-tailnet choice. Compared in SQL against `now()` so it's offset-correct.
pub fn has_live_session_blocking(db: &Db, box_id: Uuid, max_age_secs: i64) -> bool {
    let pool = db.pool();
    db.block(async move {
        sqlx::query_scalar::<_, Option<bool>>(
            "SELECT sse_last_seen IS NOT NULL
                    AND sse_last_seen::timestamptz > now() - make_interval(secs => $2::double precision)
             FROM tenant_boxes WHERE id = $1",
        )
        .bind(box_id).bind(max_age_secs as f64)
        .fetch_optional(&pool).await
    })
    .ok()
    .flatten()
    .flatten()
    .unwrap_or(false)
}

/// Await a command's terminal result, blocking. Polls the row until `done`/`failed` or `timeout`.
/// Returns the `result` JSON on success, or the `error` (or "timeout") string. Polling is fine here:
/// this runs on a flow std thread and the wait is bounded and short; the NOTIFY latency win is on
/// the *delivery* side, which is what actually needs to be instant.
pub fn await_result_blocking(db: &Db, id: Uuid, timeout_secs: u64) -> Result<serde_json::Value, String> {
    let pool = db.pool();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(timeout_secs);
    loop {
        let pool2 = pool.clone();
        let row: Option<(String, Option<serde_json::Value>, Option<String>)> = db
            .block(async move {
                sqlx::query_as("SELECT status, result, error FROM edge_commands WHERE id = $1")
                    .bind(id)
                    .fetch_optional(&pool2)
                    .await
            })
            .map_err(|e| format!("await result: {e}"))?;
        match row {
            Some((status, result, _)) if status == "done" => {
                return Ok(result.unwrap_or(serde_json::Value::Null));
            }
            Some((status, _, error)) if status == "failed" => {
                return Err(error.unwrap_or_else(|| "edge command failed".into()));
            }
            _ => {}
        }
        if std::time::Instant::now() >= deadline {
            return Err("edge command timed out (box not reachable over SSE)".into());
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
}
