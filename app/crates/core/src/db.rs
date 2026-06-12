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

use crate::alerts::{Alert, AlertLevel, Delivery};
use crate::devices::Device;
use crate::notification::NotificationTarget;
use crate::persons::{Person, PersonEmbedding, RecognitionMatch};
use crate::workflows::Graph;

const SELECT_COLS: &str =
    "id, ip, xaddr, name, manufacturer, model, username, password, rtsp_url";

// Persons: identity + hierarchy + permissions. Face biometrics live in their own
// table (`person_embeddings`), never on the person row.
const PERSON_COLS: &str = "id, name, parent_id, recipient_label, permissions";

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

    /// The saved people, ordered by name. Empty (and logged) on a query error.
    /// The `embedding` column is not selected — it's read separately for recognition.
    pub fn load_persons(&self) -> Vec<Person> {
        let pool = self.pool.clone();
        self.block(async move {
            let sql = format!("SELECT {PERSON_COLS} FROM persons ORDER BY name");
            sqlx::query_as::<_, Person>(&sql)
                .fetch_all(&pool)
                .await
                .unwrap_or_else(|e| {
                    log::error!("load_persons query failed: {e}");
                    Vec::new()
                })
        })
    }

    /// Persist the full person set: upsert each row's metadata (leaving `embedding`
    /// and `created_at` untouched, so an enrolled face survives a metadata edit),
    /// then delete any row no longer in the in-memory set. Best-effort.
    pub fn save_persons(&self, people: &[Person]) {
        let pool = self.pool.clone();
        let people = people.to_vec();
        let res: Result<(), sqlx::Error> = self.block(async move {
            let mut tx = pool.begin().await?;
            let ids: Vec<String> = people.iter().map(|p| p.id.clone()).collect();
            // Prune rows that were removed from the store.
            sqlx::query("DELETE FROM persons WHERE NOT (id = ANY($1))")
                .bind(&ids)
                .execute(&mut *tx)
                .await?;
            for p in &people {
                sqlx::query(
                    "INSERT INTO persons (id, name, parent_id, recipient_label, permissions) \
                     VALUES ($1, $2, $3, $4, $5) \
                     ON CONFLICT (id) DO UPDATE SET \
                       name = EXCLUDED.name, \
                       parent_id = EXCLUDED.parent_id, \
                       recipient_label = EXCLUDED.recipient_label, \
                       permissions = EXCLUDED.permissions",
                )
                .bind(&p.id)
                .bind(&p.name)
                .bind(&p.parent_id)
                .bind(&p.recipient_label)
                .bind(&p.permissions)
                .execute(&mut *tx)
                .await?;
            }
            tx.commit().await?;
            Ok(())
        });
        if let Err(e) = res {
            log::error!("save_persons transaction failed: {e}");
        }
    }

    /// Insert one face embedding for `person_id` (its own uuid, individual approval)
    /// and return the new row id. The vector is passed as a pgvector text literal
    /// (`[a,b,…]`) cast to `vector`, so no extra crate is needed.
    pub fn insert_embedding(
        &self,
        person_id: &str,
        embedding: &[f32],
        approved: bool,
        source: &str,
    ) -> Result<String, String> {
        let pool = self.pool.clone();
        let person_id = person_id.to_string();
        let literal = vector_literal(embedding);
        let source = source.to_string();
        self.block(async move {
            let row: (String,) = sqlx::query_as(
                "INSERT INTO person_embeddings (person_id, embedding, approved, approved_at, source) \
                 VALUES ($1, $2::vector, $3, CASE WHEN $3 THEN now() END, $4) \
                 RETURNING id::text",
            )
            .bind(&person_id)
            .bind(&literal)
            .bind(approved)
            .bind(&source)
            .fetch_one(&pool)
            .await
            .map_err(|e| format!("insert_embedding: {e}"))?;
            Ok(row.0)
        })
    }

    /// Every embedding for `person_id`, newest first (with the raw vector for the avatar).
    pub fn list_embeddings(&self, person_id: &str) -> Vec<PersonEmbedding> {
        let pool = self.pool.clone();
        let person_id = person_id.to_string();
        self.block(async move {
            sqlx::query_as::<_, PersonEmbedding>(
                "SELECT id::text AS id, person_id, approved, source, \
                        created_at::text AS created_at, embedding::float4[] AS embedding \
                 FROM person_embeddings WHERE person_id = $1 ORDER BY created_at DESC",
            )
            .bind(&person_id)
            .fetch_all(&pool)
            .await
            .unwrap_or_else(|e| {
                log::error!("list_embeddings query failed: {e}");
                Vec::new()
            })
        })
    }

    /// Approve one embedding (sets `approved` + `approved_at`).
    pub fn approve_embedding(&self, id: &str) -> Result<(), String> {
        let pool = self.pool.clone();
        let id = id.to_string();
        self.block(async move {
            sqlx::query(
                "UPDATE person_embeddings SET approved = true, approved_at = now() WHERE id = $1::uuid",
            )
            .bind(&id)
            .execute(&pool)
            .await
            .map(|_| ())
            .map_err(|e| format!("approve_embedding: {e}"))
        })
    }

    /// Delete one embedding by id.
    pub fn delete_embedding(&self, id: &str) -> Result<(), String> {
        let pool = self.pool.clone();
        let id = id.to_string();
        self.block(async move {
            sqlx::query("DELETE FROM person_embeddings WHERE id = $1::uuid")
                .bind(&id)
                .execute(&pool)
                .await
                .map(|_| ())
                .map_err(|e| format!("delete_embedding: {e}"))
        })
    }

    /// Nearest people to `embedding` by cosine distance over **approved** embeddings,
    /// closest first (≤ `k` rows). A person can appear more than once (one row per
    /// matching sample); the caller aggregates. Empty on error.
    pub fn recognize(&self, embedding: &[f32], k: i64) -> Vec<RecognitionMatch> {
        let pool = self.pool.clone();
        let literal = vector_literal(embedding);
        self.block(async move {
            let t = std::time::Instant::now();
            let rows = sqlx::query_as::<_, RecognitionMatch>(
                "SELECT pe.person_id AS person_id, p.name AS name, \
                        (pe.embedding <=> $1::vector)::float4 AS distance \
                 FROM person_embeddings pe JOIN persons p ON p.id = pe.person_id \
                 WHERE pe.approved ORDER BY distance LIMIT $2",
            )
            .bind(&literal)
            .bind(k)
            .fetch_all(&pool)
            .await
            .unwrap_or_else(|e| {
                log::error!("recognize query failed: {e}");
                Vec::new()
            });
            log::info!("recognize: pgvector kNN k={k} → {} rows in {:?}", rows.len(), t.elapsed());
            rows
        })
    }

    // ---- Alerts (Phase 0: alerts → Postgres) -------------------------------

    /// The saved alert log, oldest id first (matching the in-memory append order).
    /// Empty (and logged) on a query error — best-effort, like `load_devices`.
    pub fn load_alerts(&self) -> Vec<Alert> {
        let pool = self.pool.clone();
        self.block(async move {
            type Row = (i64, String, String, Option<i64>, Option<String>, String, String, serde_json::Value);
            let rows = sqlx::query_as::<_, Row>(
                "SELECT id, level, message, room_id, room_name, source, created_at, deliveries \
                 FROM alerts ORDER BY id",
            )
            .fetch_all(&pool)
            .await;
            match rows {
                Ok(rows) => rows
                    .into_iter()
                    .map(|(id, level, message, room_id, room_name, source, created_at, deliveries)| Alert {
                        id: id as u64,
                        level: AlertLevel::parse(&level).unwrap_or_default(),
                        message,
                        room_id: room_id.map(|r| r as u32),
                        room_name,
                        source,
                        created_at,
                        deliveries: serde_json::from_value::<Vec<Delivery>>(deliveries).unwrap_or_default(),
                    })
                    .collect(),
                Err(e) => {
                    log::error!("load_alerts query failed: {e}");
                    Vec::new()
                }
            }
        })
    }

    /// Insert one alert row (id is app-assigned). Best-effort; errors are logged.
    pub fn insert_alert(&self, a: &Alert) {
        let pool = self.pool.clone();
        let id = a.id as i64;
        let level = a.level.label().to_ascii_lowercase();
        let message = a.message.clone();
        let room_id = a.room_id.map(|r| r as i64);
        let room_name = a.room_name.clone();
        let source = a.source.clone();
        let created_at = a.created_at.clone();
        let deliveries = serde_json::to_value(&a.deliveries).unwrap_or_else(|_| serde_json::json!([]));
        let res: Result<(), sqlx::Error> = self.block(async move {
            sqlx::query(
                "INSERT INTO alerts (id, level, message, room_id, room_name, source, created_at, deliveries) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8) ON CONFLICT (id) DO NOTHING",
            )
            .bind(id)
            .bind(&level)
            .bind(&message)
            .bind(room_id)
            .bind(&room_name)
            .bind(&source)
            .bind(&created_at)
            .bind(&deliveries)
            .execute(&pool)
            .await
            .map(|_| ())
        });
        if let Err(e) = res {
            log::error!("insert_alert failed: {e}");
        }
    }

    /// Delete one alert row by id. Best-effort.
    pub fn delete_alert(&self, id: u64) {
        let pool = self.pool.clone();
        let id = id as i64;
        let res: Result<(), sqlx::Error> = self.block(async move {
            sqlx::query("DELETE FROM alerts WHERE id = $1").bind(id).execute(&pool).await.map(|_| ())
        });
        if let Err(e) = res {
            log::error!("delete_alert failed: {e}");
        }
    }

    /// Replace an alert's delivery list (after a deliver run). Best-effort.
    pub fn update_alert_deliveries(&self, id: u64, deliveries: &[Delivery]) {
        let pool = self.pool.clone();
        let id = id as i64;
        let json = serde_json::to_value(deliveries).unwrap_or_else(|_| serde_json::json!([]));
        let res: Result<(), sqlx::Error> = self.block(async move {
            sqlx::query("UPDATE alerts SET deliveries = $2 WHERE id = $1")
                .bind(id)
                .bind(&json)
                .execute(&pool)
                .await
                .map(|_| ())
        });
        if let Err(e) = res {
            log::error!("update_alert_deliveries failed: {e}");
        }
    }

    // ---- Notification targets / beacons (Phase 0) --------------------------

    /// The saved beacons, ordered by label. Empty (and logged) on a query error.
    pub fn load_beacons(&self) -> Vec<NotificationTarget> {
        let pool = self.pool.clone();
        self.block(async move {
            sqlx::query_as::<_, (i64, String, bool)>(
                "SELECT chat_id, label, enabled FROM notification_targets ORDER BY label",
            )
            .fetch_all(&pool)
            .await
            .map(|rows| {
                rows.into_iter()
                    .map(|(chat_id, label, enabled)| NotificationTarget { label, chat_id, enabled })
                    .collect()
            })
            .unwrap_or_else(|e| {
                log::error!("load_beacons query failed: {e}");
                Vec::new()
            })
        })
    }

    /// Replace the whole beacon set (delete-all + insert each), mirroring the
    /// in-memory store's "full list after every mutation" semantics. Best-effort.
    pub fn save_beacons(&self, beacons: &[NotificationTarget]) {
        let pool = self.pool.clone();
        let beacons = beacons.to_vec();
        let res: Result<(), sqlx::Error> = self.block(async move {
            let mut tx = pool.begin().await?;
            sqlx::query("DELETE FROM notification_targets").execute(&mut *tx).await?;
            for b in &beacons {
                sqlx::query(
                    "INSERT INTO notification_targets (chat_id, label, enabled) VALUES ($1, $2, $3)",
                )
                .bind(b.chat_id)
                .bind(&b.label)
                .bind(b.enabled)
                .execute(&mut *tx)
                .await?;
            }
            tx.commit().await?;
            Ok(())
        });
        if let Err(e) = res {
            log::error!("save_beacons transaction failed: {e}");
        }
    }

    // ---- Workflow graphs (Phase 0) -----------------------------------------

    /// The saved workflow graphs, ordered by id. Rows that fail to deserialize are
    /// skipped (logged). Empty on a query error.
    pub fn load_workflows(&self) -> Vec<Graph> {
        let pool = self.pool.clone();
        self.block(async move {
            let rows = sqlx::query_as::<_, (serde_json::Value,)>(
                "SELECT data FROM workflows ORDER BY id",
            )
            .fetch_all(&pool)
            .await;
            match rows {
                Ok(rows) => rows
                    .into_iter()
                    .filter_map(|(v,)| match serde_json::from_value::<Graph>(v) {
                        Ok(g) => Some(g),
                        Err(e) => {
                            log::error!("load_workflows: skipping a malformed graph row: {e}");
                            None
                        }
                    })
                    .collect(),
                Err(e) => {
                    log::error!("load_workflows query failed: {e}");
                    Vec::new()
                }
            }
        })
    }

    /// Replace the whole graph set (delete-all + insert each as a JSON row),
    /// mirroring the in-memory store after every editor mutation. Best-effort.
    pub fn save_workflows(&self, graphs: &[Graph]) {
        let pool = self.pool.clone();
        let graphs = graphs.to_vec();
        let res: Result<(), sqlx::Error> = self.block(async move {
            let mut tx = pool.begin().await?;
            sqlx::query("DELETE FROM workflows").execute(&mut *tx).await?;
            for g in &graphs {
                let data = serde_json::to_value(g).unwrap_or_else(|_| serde_json::json!({}));
                sqlx::query("INSERT INTO workflows (id, data) VALUES ($1, $2)")
                    .bind(g.id as i64)
                    .bind(&data)
                    .execute(&mut *tx)
                    .await?;
            }
            tx.commit().await?;
            Ok(())
        });
        if let Err(e) = res {
            log::error!("save_workflows transaction failed: {e}");
        }
    }

    /// Ids of every person with ≥1 approved embedding — drives the "enrolled" badge.
    pub fn enrolled_person_ids(&self) -> Vec<String> {
        let pool = self.pool.clone();
        self.block(async move {
            sqlx::query_as::<_, (String,)>(
                "SELECT DISTINCT person_id FROM person_embeddings WHERE approved",
            )
            .fetch_all(&pool)
            .await
            .map(|rows| rows.into_iter().map(|r| r.0).collect())
            .unwrap_or_else(|e| {
                log::error!("enrolled_person_ids query failed: {e}");
                Vec::new()
            })
        })
    }
}

/// Format an embedding as a pgvector text literal: `[0.1,0.2,…]`.
fn vector_literal(embedding: &[f32]) -> String {
    let mut s = String::with_capacity(embedding.len() * 8 + 2);
    s.push('[');
    for (i, v) in embedding.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        s.push_str(&v.to_string());
    }
    s.push(']');
    s
}
