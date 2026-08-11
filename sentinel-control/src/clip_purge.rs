//! Consumers for the `clip_delete` work-queue category (card #64, Phase 6).
//!
//! **Design contract.** The only place that knows a clip deletion is *three* actions is where
//! they are enqueued (`domain::recordings::enqueue_delete`). Each consumer here is an independent,
//! named leg that knows **only its own end** — the cloud consumer knows R2, the edge consumer
//! knows the edge, the db consumer knows the row. None knows the others exist. That keeps failures
//! isolated (a stuck leg is one named queue, easy to spot in monitoring) and the system simple to
//! reason about: to add a fourth action, enqueue a fourth message and add a fourth handler — no
//! existing leg changes.
//!
//! Each consumer wakes on `LISTEN work_jobs` (the producer `NOTIFY`s on enqueue) and, as a
//! fallback, on a slow poll — which also picks up retries and any NOTIFY missed while it was down.
//! Handlers are idempotent: re-deleting something already gone is success. A consumer that is down
//! simply leaves its messages `pending`; they run when it returns.

use std::future::Future;
use std::time::Duration;

use sqlx::postgres::PgListener;
use sqlx::PgPool;

use crate::domain::recordings::{
    edge_api_base, normalize_host, DELETE_CATEGORY, KIND_CLOUD, KIND_DB, KIND_EDGE,
};
use app_core::work_queue::{self, Job};

/// Retries before a job is parked `failed`. A `failed` leg is a visible gap in monitoring, not a
/// blocker on the others — the queue keeps the message and its payload for a later re-run.
const MAX_ATTEMPTS: i32 = 8;

/// Fallback poll when no NOTIFY arrives — catches retries and missed wakeups. NOTIFY makes the
/// common case immediate; this only bounds the worst case.
const POLL: Duration = Duration::from_secs(30);

/// How many jobs a consumer claims per pass.
const BATCH: i64 = 50;

/// How long a "not ready" job is held off the claim before the next attempt (store offline, etc).
const RELEASE_DELAY_SECS: i64 = 30;

/// Start the three clip-delete consumers, each named for the leg it drains.
pub fn spawn(pool: PgPool) {
    spawn_consumer(
        pool.clone(),
        "clip-delete-cloud",
        KIND_CLOUD,
        delete_from_cloud,
    );
    spawn_consumer(
        pool.clone(),
        "clip-delete-edge",
        KIND_EDGE,
        delete_from_edge,
    );
    spawn_consumer(pool, "clip-delete-db", KIND_DB, delete_from_db);
    log::info!("clip-delete consumers started: cloud, edge, db");
}

// ---- generic infrastructure (knows nothing about deletion) ------------------

fn spawn_consumer<F, Fut>(pool: PgPool, name: &'static str, kind: &'static str, handler: F)
where
    F: Fn(PgPool, Job) -> Fut + Copy + Send + 'static,
    Fut: Future<Output = Result<bool, String>> + Send,
{
    tokio::spawn(async move { consumer_loop(pool, name, kind, handler).await });
}

async fn consumer_loop<F, Fut>(pool: PgPool, name: &'static str, kind: &'static str, handler: F)
where
    F: Fn(PgPool, Job) -> Fut + Copy + Send + 'static,
    Fut: Future<Output = Result<bool, String>> + Send,
{
    loop {
        // A dropped LISTEN connection (DB restart) must not kill the consumer — reconnect, and the
        // poll keeps it draining meanwhile.
        let mut listener = match connect_listener(&pool).await {
            Ok(l) => l,
            Err(e) => {
                log::warn!("[{name}] listen unavailable ({e}); polling only");
                drain(&pool, name, kind, handler).await;
                tokio::time::sleep(POLL).await;
                continue;
            }
        };
        loop {
            drain(&pool, name, kind, handler).await;
            match tokio::time::timeout(POLL, listener.recv()).await {
                Ok(Ok(_)) => {} // notified — loop and drain
                Ok(Err(e)) => {
                    log::warn!("[{name}] listener dropped ({e}); reconnecting");
                    break;
                }
                Err(_) => {} // poll timeout — loop and drain
            }
        }
    }
}

/// Open a LISTEN connection on the work-queue channel.
async fn connect_listener(pool: &PgPool) -> Result<PgListener, sqlx::Error> {
    let mut l = PgListener::connect_with(pool).await?;
    l.listen(work_queue::CHANNEL).await?;
    Ok(l)
}

async fn drain<F, Fut>(pool: &PgPool, name: &str, kind: &str, handler: F)
where
    F: Fn(PgPool, Job) -> Fut + Copy,
    Fut: Future<Output = Result<bool, String>>,
{
    loop {
        let jobs = match work_queue::claim(pool, DELETE_CATEGORY, kind, BATCH).await {
            Ok(j) if !j.is_empty() => j,
            Ok(_) => return,
            Err(e) => {
                log::warn!("[{name}] claim failed: {e}");
                return;
            }
        };
        for job in jobs {
            let id = job.id;
            match handler(pool.clone(), job).await {
                Ok(true) => {
                    let _ = work_queue::mark_done(pool, id).await;
                }
                // Not ready (e.g. store offline) — the claim flipped it to `running`, so hand it
                // back to `pending` explicitly (with a short hold) rather than leave it stuck.
                Ok(false) => {
                    let _ = work_queue::release(pool, id, RELEASE_DELAY_SECS).await;
                }
                Err(e) => {
                    log::warn!("[{name}] job {id}: {e}");
                    let _ = work_queue::mark_failed(pool, id, &e, MAX_ATTEMPTS).await;
                }
            }
        }
    }
}

// ---- the three legs — each knows only its own end ---------------------------

/// Delete the durable copy from R2. Knows only the object store. Idempotent: a 404 is success.
async fn delete_from_cloud(_pool: PgPool, job: Job) -> Result<bool, String> {
    let Some(key) = job
        .payload
        .get("cloud_key")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
    else {
        return Ok(true); // this clip never had a cloud object
    };
    // Store not configured (R2 down / unset): don't burn attempts failing — leave it pending so it
    // runs when the store is back. The named queue holds the message meanwhile.
    if !app_core::clip_store::enabled() {
        return Ok(false);
    }
    let key = key.to_string();
    // clip_store::delete drives its own runtime (blocking) → spawn_blocking.
    tokio::task::spawn_blocking(move || app_core::clip_store::delete(&key))
        .await
        .map_err(|e| format!("task: {e}"))?
        .map(|()| true)
}

/// Delete the local file from the tenant's edge box. Knows only the edge.
/// Idempotent: a 404 from the edge means it's already gone.
///
/// SSE first (card #144), tailnet second — the same order as the Devices probe. Without the SSE leg
/// this job did something worse than fail: `box_host_for_tenant` is NULL for every outbound-only
/// box, so it took the "no box connected — nothing to delete here" branch and reported SUCCESS
/// while the file stayed on the appliance forever. A delete that frees no space and says it worked
/// is how a disk fills up with clips the product believes are gone.
async fn delete_from_edge(pool: PgPool, job: Job) -> Result<bool, String> {
    let Some(clip_id) = job.payload.get("clip_id").and_then(|v| v.as_str()) else {
        return Ok(true);
    };
    let path = format!("/recording/{}", urlencoding::encode(clip_id));

    if let Some(res) =
        app_core::edge_commands::edge_http_via_sse(&pool, job.tenant_id, "DELETE", &path, None, 30)
            .await
    {
        let (status, _) = res.map_err(|e| format!("edge delete over SSE: {e}"))?;
        // 404 = already gone, which is success for an idempotent delete.
        if (200..300).contains(&status) || status == 404 {
            return Ok(true);
        }
        return Err(format!("edge returned {status}"));
    }

    let Some(host) = crate::store::box_host_for_tenant(&pool, job.tenant_id)
        .await
        .map_err(|e| e.to_string())?
    else {
        return Ok(true); // no box on SSE and none on the tailnet — nothing reachable to delete
    };
    let base = edge_api_base(&normalize_host(&host));
    let url = format!("{base}{path}");
    match reqwest::Client::new().delete(&url).send().await {
        Ok(r) if r.status().is_success() || r.status() == reqwest::StatusCode::NOT_FOUND => {
            Ok(true)
        }
        Ok(r) => Err(format!("edge returned {}", r.status())),
        Err(e) => Err(format!("edge unreachable: {e}")),
    }
}

/// Delete the metadata row. Knows only the DB. Independent of the other legs — the queue, not the
/// row, is the durable pointer, so the row can go while cloud/edge finish on their own.
/// Idempotent: deleting an already-gone row affects zero rows, still success.
async fn delete_from_db(pool: PgPool, job: Job) -> Result<bool, String> {
    let Some(rec_id) = job
        .payload
        .get("recording_id")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse::<i64>().ok())
    else {
        return Ok(true);
    };
    crate::store::delete_recording(&pool, job.tenant_id, rec_id)
        .await
        .map_err(|e| e.to_string())?;
    Ok(true)
}
