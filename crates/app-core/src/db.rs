//! Shared Postgres **infrastructure** — the connection pool, the dedicated runtime,
//! and the sync bridge. It is deliberately feature-agnostic: each feature slice owns
//! its own SQL in a `<feature>/repo.rs` that borrows this handle. Nothing here
//! `use`s a domain type.
//!
//! The rest of the app is synchronous — Tauri commands and agent tool executors are
//! plain `fn`s — but sqlx is async. Rather than colour the whole call graph `async`,
//! [`Db`] owns a multi-thread tokio runtime and exposes a synchronous [`Db::block`]:
//! each call hands its future to that runtime and waits on a channel, so it's safe
//! from any thread (a sync command thread, the Tauri setup hook, or even a Tauri
//! async worker — we never `block_on` the *current* thread, sidestepping the
//! "runtime within a runtime" panic).
//!
//! Queries (in the feature repos) are runtime (`query_as` + `FromRow`), not the
//! compile-checked `query!` macro, so the build needs no live database.
use std::sync::Arc;

use sqlx::postgres::{PgPool, PgPoolOptions};

/// A handle to the Postgres pool plus the runtime that drives it. Cheap to clone
/// (shares the pool and the runtime) and stored in the Tauri managed state. Feature
/// repos take `&Db` and call [`Db::pool`] + [`Db::block`] to run their own SQL.
#[derive(Clone)]
pub struct Db {
    pool: PgPool,
    rt: Arc<tokio::runtime::Runtime>,
}

impl Db {
    /// Connect to `database_url` and apply the embedded migrations. Returns an
    /// error if the database is unreachable or a migration fails — the caller
    /// (the splash boot loop) retries.
    pub fn connect(database_url: &str) -> Result<Db, sqlx::Error> {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .map_err(sqlx::Error::Io)?;

        let url = database_url.to_string();
        let pool = rt.block_on(async move {
            let pool = PgPoolOptions::new()
                .max_connections(5)
                .connect(&url)
                .await?;
            sqlx::migrate!("./migrations").run(&pool).await?;
            Ok::<_, sqlx::Error>(pool)
        })?;

        Ok(Db { pool, rt: Arc::new(rt) })
    }

    /// A `Db` whose pool connects lazily (on first query) and runs no migrations.
    /// For unit tests that build a [`crate::agent::ToolContext`] but never touch a
    /// repo — nothing connects unless a query actually runs.
    #[cfg(test)]
    pub fn lazy(database_url: &str) -> Db {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .expect("build test runtime");
        // connect_lazy spawns the pool's maintenance task, so it must run inside the
        // runtime context even though it doesn't actually connect.
        let pool = {
            let _guard = rt.enter();
            PgPoolOptions::new()
                // Fail fast when the (test) DB is unreachable instead of stalling on the
                // 30s default — best-effort writes return quickly, well within a node's
                // watchdog deadline, so timeout tests stay fast and deterministic.
                .acquire_timeout(std::time::Duration::from_secs(3))
                .connect_lazy(database_url)
                .expect("lazy pool")
        };
        Db { pool, rt: Arc::new(rt) }
    }

    /// The connection pool (cheap clone). Feature repos use this to run their own
    /// SQL — keeping per-feature queries out of this shared infra module.
    pub(crate) fn pool(&self) -> PgPool {
        self.pool.clone()
    }

    /// Drive `fut` to completion on the owned runtime and block the calling thread
    /// until it finishes. Safe from inside another runtime (we `spawn`, not
    /// `block_on`, on the current thread). `pub(crate)` so feature repos can reuse
    /// the runtime without re-implementing the sync bridge.
    pub(crate) fn block<T, F>(&self, fut: F) -> T
    where
        F: std::future::Future<Output = T> + Send + 'static,
        T: Send + 'static,
    {
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.rt.spawn(async move {
            let _ = tx.send(fut.await);
        });
        rx.recv().expect("db runtime task dropped before completing")
    }

    /// Fire a Postgres `NOTIFY <channel>, <payload>` (best-effort, blocking). The workflow
    /// live-event fabric (card #147, replacing Redis): every flow pod notifies `workflow_events`,
    /// control's SSE relay LISTENs — fans out across all pods with no broker. Payload must stay
    /// under Postgres's ~8000-byte NOTIFY limit; oversized ones are dropped (telemetry is lossy).
    /// Blocks the caller on the owned runtime, so call from a std thread or `spawn_blocking`.
    pub fn notify(&self, channel: &str, payload: &str) {
        if payload.len() >= 7900 {
            return;
        }
        let pool = self.pool();
        let channel = channel.to_string();
        let payload = payload.to_string();
        let _ = self.block(async move {
            sqlx::query("SELECT pg_notify($1, $2)")
                .bind(&channel)
                .bind(&payload)
                .execute(&pool)
                .await
        });
    }
}
