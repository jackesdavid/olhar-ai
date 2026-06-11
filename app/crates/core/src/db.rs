//! Postgres persistence for the devices module.
//!
//! The rest of the app is synchronous — Tauri commands and agent tool executors
//! are plain `fn`s — but sqlx is async. Rather than colour the whole call graph
//! `async`, [`Db`] owns a dedicated multi-thread tokio runtime and exposes
//! **synchronous** `load_devices` / `save_devices`. Each call hands its future to
//! that runtime via [`Db::block`] and waits on a channel, so it's safe to call from
//! any thread (a sync command thread, the Tauri setup hook, or even a Tauri async
//! worker — we never call `block_on` on the *current* thread, sidestepping the
//! "runtime within a runtime" panic).
//!
//! Queries are runtime (`query_as` + `FromRow`), not the compile-checked `query!`
//! macro, so the build needs no live database or `DATABASE_URL`.
use std::sync::Arc;

use sqlx::postgres::{PgPool, PgPoolOptions};

use crate::devices::Device;

const SELECT_COLS: &str =
    "id, ip, xaddr, name, manufacturer, model, username, password, rtsp_url";

/// A handle to the Postgres pool plus the runtime that drives it. Cheap to clone
/// (shares the pool and the runtime) and stored in the Tauri managed state.
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
    /// For unit tests that build a [`crate::agent::ToolContext`] but never touch
    /// the device store — nothing connects unless a query actually runs.
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
                .connect_lazy(database_url)
                .expect("lazy pool")
        };
        Db { pool, rt: Arc::new(rt) }
    }

    /// Drive `fut` to completion on the owned runtime and block the calling thread
    /// until it finishes. Safe from inside another runtime (we `spawn`, not
    /// `block_on`, on the current thread).
    fn block<T, F>(&self, fut: F) -> T
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

    /// The saved device list, ordered by name. Returns an empty list (and logs)
    /// on a query error — matching the best-effort behaviour of the old JSON load.
    pub fn load_devices(&self) -> Vec<Device> {
        let pool = self.pool.clone();
        self.block(async move {
            let sql = format!("SELECT {SELECT_COLS} FROM devices ORDER BY name");
            sqlx::query_as::<_, Device>(&sql)
                .fetch_all(&pool)
                .await
                .unwrap_or_else(|e| {
                    log::error!("load_devices query failed: {e}");
                    Vec::new()
                })
        })
    }

    /// Replace the whole device set in one transaction (delete-all + insert each).
    /// The in-memory `DeviceStore` is the authoritative full list after every
    /// mutation, so replace-all mirrors the old "write the whole file" semantics.
    /// Best-effort: errors are logged, not propagated.
    pub fn save_devices(&self, devices: &[Device]) {
        let pool = self.pool.clone();
        let devices = devices.to_vec();
        let res: Result<(), sqlx::Error> = self.block(async move {
            let mut tx = pool.begin().await?;
            sqlx::query("DELETE FROM devices").execute(&mut *tx).await?;
            for d in &devices {
                sqlx::query(
                    "INSERT INTO devices \
                     (ip, id, xaddr, name, manufacturer, model, username, password, rtsp_url) \
                     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
                )
                .bind(&d.ip)
                .bind(&d.id)
                .bind(&d.xaddr)
                .bind(&d.name)
                .bind(&d.manufacturer)
                .bind(&d.model)
                .bind(&d.username)
                .bind(&d.password)
                .bind(&d.rtsp_url)
                .execute(&mut *tx)
                .await?;
            }
            tx.commit().await?;
            Ok(())
        });
        if let Err(e) = res {
            log::error!("save_devices transaction failed: {e}");
        }
    }
}
