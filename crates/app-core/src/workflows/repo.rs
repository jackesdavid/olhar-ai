//! Postgres adapter for the workflows slice — the persistence port for graphs.
//!
//! This owns the workflow SQL (moved out of the shared `db.rs`): the domain
//! (`Graph`) stays pure, and this adapter is the only place that touches the DB on
//! its behalf. Functions take the shared [`Db`] infra handle (pool + runtime).
use uuid::Uuid;

use crate::db::Db;

use super::domain::Graph;

/// On-disk shape of the legacy `workflows.json` — one object holding the graph array.
#[derive(serde::Deserialize)]
struct WorkflowsFile {
    #[serde(default)]
    graphs: Vec<Graph>,
}

/// Read graphs from a legacy `workflows.json` (empty if missing/unreadable). Used
/// only for the one-time JSON→Postgres import on first DB boot. This is a
/// persistence-adapter concern, so it lives here, not in the pure domain.
pub fn import_legacy_json(path: &std::path::Path) -> Vec<Graph> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str::<WorkflowsFile>(&s).ok())
        .map(|f| f.graphs)
        .unwrap_or_default()
}

/// The tenant's workflow graphs, ordered by id. Rows that fail to deserialize are
/// skipped (logged). Empty on a query error.
pub fn load(db: &Db, tenant_id: Uuid) -> Vec<Graph> {
    let pool = db.pool();
    db.block(async move {
        let rows = sqlx::query_as::<_, (serde_json::Value,)>(
            "SELECT data FROM workflows WHERE tenant_id = $1 ORDER BY id",
        )
        .bind(tenant_id)
        .fetch_all(&pool)
        .await;
        match rows {
            Ok(rows) => rows
                .into_iter()
                .filter_map(|(v,)| match serde_json::from_value::<Graph>(v) {
                    Ok(g) => Some(g),
                    Err(e) => {
                        log::error!("workflows::repo::load: skipping a malformed graph row: {e}");
                        None
                    }
                })
                .collect(),
            Err(e) => {
                log::error!("workflows::repo::load query failed: {e}");
                Vec::new()
            }
        }
    })
}

/// EVERY tenant's graphs as `(tenant_id, Graph)`, ordered by tenant then id — for a
/// multi-tenant flow pod that polls/runs all tenants. Malformed rows are skipped.
pub fn load_all(db: &Db) -> Vec<(Uuid, Graph)> {
    let pool = db.pool();
    db.block(async move {
        let rows = sqlx::query_as::<_, (Uuid, serde_json::Value)>(
            "SELECT tenant_id, data FROM workflows ORDER BY tenant_id, id",
        )
        .fetch_all(&pool)
        .await;
        match rows {
            Ok(rows) => rows
                .into_iter()
                .filter_map(|(tid, v)| match serde_json::from_value::<Graph>(v) {
                    Ok(g) => Some((tid, g)),
                    Err(e) => {
                        log::error!("workflows::repo::load_all: skipping a malformed graph row: {e}");
                        None
                    }
                })
                .collect(),
            Err(e) => {
                log::error!("workflows::repo::load_all query failed: {e}");
                Vec::new()
            }
        }
    })
}

/// Replace the whole graph set (delete-all + insert each as a JSON row), mirroring
/// the in-memory store after every editor mutation. Best-effort.
pub fn save(db: &Db, tenant_id: Uuid, graphs: &[Graph]) {
    let pool = db.pool();
    let graphs = graphs.to_vec();
    let res: Result<(), sqlx::Error> = db.block(async move {
        let mut tx = pool.begin().await?;
        sqlx::query("DELETE FROM workflows WHERE tenant_id = $1").bind(tenant_id).execute(&mut *tx).await?;
        for g in &graphs {
            let data = serde_json::to_value(g).unwrap_or_else(|_| serde_json::json!({}));
            sqlx::query("INSERT INTO workflows (tenant_id, id, data) VALUES ($1, $2, $3)")
                .bind(tenant_id)
                .bind(g.id as i64)
                .bind(&data)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(())
    });
    if let Err(e) = res {
        log::error!("workflows::repo::save transaction failed: {e}");
    }
}

// ---- engine state (Phase 0.5) ----------------------------------------------

/// The persisted new-alert high-water mark, or `None` if it was never written
/// (first boot) or on a query error. The adapter falls back to the current alert
/// max when this is `None` so an existing backlog still doesn't fire on first run.
pub fn load_alert_seen_max(db: &Db, tenant_id: Uuid) -> Option<u64> {
    let pool = db.pool();
    db.block(async move {
        sqlx::query_as::<_, (i64,)>("SELECT alert_seen_max FROM workflow_engine_state WHERE tenant_id = $1")
            .bind(tenant_id)
            .fetch_optional(&pool)
            .await
            .unwrap_or_else(|e| {
                log::error!("workflows::repo::load_alert_seen_max query failed: {e}");
                None
            })
            .map(|(v,)| v as u64)
    })
}

/// Upsert the tenant's new-alert high-water mark. Best-effort.
pub fn save_alert_seen_max(db: &Db, tenant_id: Uuid, max: u64) {
    let pool = db.pool();
    let res: Result<_, sqlx::Error> = db.block(async move {
        sqlx::query(
            "INSERT INTO workflow_engine_state (tenant_id, id, alert_seen_max) VALUES ($1, true, $2)
             ON CONFLICT (tenant_id) DO UPDATE SET alert_seen_max = EXCLUDED.alert_seen_max",
        )
        .bind(tenant_id)
        .bind(max as i64)
        .execute(&pool)
        .await
    });
    if let Err(e) = res {
        log::error!("workflows::repo::save_alert_seen_max failed: {e}");
    }
}

/// The tenant's per-graph cron timers as `(graph_id, last_fired_ms)`. Empty on error.
pub fn load_cron_state(db: &Db, tenant_id: Uuid) -> Vec<(u32, u64)> {
    let pool = db.pool();
    db.block(async move {
        sqlx::query_as::<_, (i64, i64)>(
            "SELECT graph_id, last_fired_ms FROM workflow_cron_state WHERE tenant_id = $1",
        )
        .bind(tenant_id)
        .fetch_all(&pool)
        .await
        .unwrap_or_else(|e| {
            log::error!("workflows::repo::load_cron_state query failed: {e}");
            Vec::new()
        })
        .into_iter()
        .map(|(id, ms)| (id as u32, ms as u64))
        .collect()
    })
}

/// Record that the tenant's cron graph `graph_id` fired at `fired_ms`. Best-effort.
pub fn save_cron_fired(db: &Db, tenant_id: Uuid, graph_id: u32, fired_ms: u64) {
    let pool = db.pool();
    let res: Result<_, sqlx::Error> = db.block(async move {
        sqlx::query(
            "INSERT INTO workflow_cron_state (tenant_id, graph_id, last_fired_ms) VALUES ($1, $2, $3)
             ON CONFLICT (tenant_id, graph_id) DO UPDATE SET last_fired_ms = EXCLUDED.last_fired_ms",
        )
        .bind(tenant_id)
        .bind(graph_id as i64)
        .bind(fired_ms as i64)
        .execute(&pool)
        .await
    });
    if let Err(e) = res {
        log::error!("workflows::repo::save_cron_fired failed: {e}");
    }
}
