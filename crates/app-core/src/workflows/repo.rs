//! Postgres adapter for the workflows slice — the persistence port for graphs.
//!
//! This owns the workflow SQL (moved out of the shared `db.rs`): the domain
//! (`Graph`) stays pure, and this adapter is the only place that touches the DB on
//! its behalf. Functions take the shared [`Db`] infra handle (pool + runtime).
use uuid::Uuid;

use crate::db::Db;

use super::domain::{Graph, PortType, RunStatus, TriggerSource};

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

/// Returns the persisted JSON state for a node, or `None` if never set.
pub fn get_node_state(db: &Db, tenant_id: Uuid, graph_id: u32, node_id: u32) -> Option<serde_json::Value> {
    let pool = db.pool();
    db.block(async move {
        sqlx::query_as::<_, (String,)>(
            "SELECT state_json FROM workflow_node_state WHERE tenant_id = $1 AND graph_id = $2 AND node_id = $3",
        )
        .bind(tenant_id)
        .bind(graph_id as i64)
        .bind(node_id as i64)
        .fetch_optional(&pool)
        .await
        .ok()
        .flatten()
        .and_then(|(s,)| serde_json::from_str(&s).ok())
    })
}

/// Upserts the JSON state for a node. Best-effort (errors are logged, not propagated).
pub fn set_node_state(db: &Db, tenant_id: Uuid, graph_id: u32, node_id: u32, state: &serde_json::Value) {
    let pool = db.pool();
    let json_str = state.to_string();
    let res: Result<_, sqlx::Error> = db.block(async move {
        sqlx::query(
            "INSERT INTO workflow_node_state (tenant_id, graph_id, node_id, state_json) VALUES ($1, $2, $3, $4)
             ON CONFLICT (tenant_id, graph_id, node_id) DO UPDATE SET state_json = EXCLUDED.state_json",
        )
        .bind(tenant_id)
        .bind(graph_id as i64)
        .bind(node_id as i64)
        .bind(json_str)
        .execute(&pool)
        .await
    });
    if let Err(e) = res {
        log::error!("workflows::repo::set_node_state failed: {e}");
    }
}

// ---- event registry + categories (per tenant) ------------------------------

/// A tenant-scoped event category. `source` is `user` or `system`.
#[derive(Debug, Clone, serde::Serialize)]
pub struct EventCategory {
    pub name: String,
    pub source: String,
}

/// One typed field an event carries. `EmitEvent` gets an input port per field; the
/// matching `OnEvent` an output port per field. Serializes as `{ "name", "type" }`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EventField {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: PortType,
}

/// A registered event other graphs can emit/listen for, grouped by `category`.
#[derive(Debug, Clone, serde::Serialize)]
pub struct TenantEvent {
    pub name: String,
    pub category: String,
    pub source: String,
    pub description: Option<String>,
    pub created_by_graph: Option<u32>,
    /// The typed payload schema (possibly empty).
    pub fields: Vec<EventField>,
}

/// Platform-defined events available to EVERY tenant, with their payload schema. Merged into
/// the event list the editor fetches, so `OnEvent` nodes expose typed outputs immediately —
/// no per-tenant registration and no first-event chicken-and-egg. System events are emitted
/// by the platform (e.g. `motion` by the edge), not user `EmitEvent` nodes. Add new ones here.
pub fn system_events() -> Vec<TenantEvent> {
    let field = |name: &str, ty: PortType| EventField { name: name.to_string(), ty };
    vec![TenantEvent {
        name: "motion".to_string(),
        category: "System".to_string(),
        source: "system".to_string(),
        description: Some("Edge motion detection".to_string()),
        created_by_graph: None,
        fields: vec![
            field("camera", PortType::Text),
            field("score", PortType::Num),
            field("area", PortType::Num),
            field("blobs", PortType::Num),
            field("bbox", PortType::NumArray),
            field("frame", PortType::Image),
        ],
    }]
}

/// The tenant's event categories, ordered by name. Empty on error.
pub fn list_categories(db: &Db, tenant_id: Uuid) -> Vec<EventCategory> {
    let pool = db.pool();
    db.block(async move {
        sqlx::query_as::<_, (String, String)>(
            "SELECT name, source FROM tenant_event_categories WHERE tenant_id = $1 ORDER BY name",
        )
        .bind(tenant_id)
        .fetch_all(&pool)
        .await
        .unwrap_or_else(|e| {
            log::error!("workflows::repo::list_categories query failed: {e}");
            Vec::new()
        })
        .into_iter()
        .map(|(name, source)| EventCategory { name, source })
        .collect()
    })
}

/// Create a tenant event category (idempotent). Best-effort.
pub fn create_category(db: &Db, tenant_id: Uuid, name: &str, source: &str) {
    let pool = db.pool();
    let name = name.to_string();
    let source = source.to_string();
    let res: Result<_, sqlx::Error> = db.block(async move {
        sqlx::query(
            "INSERT INTO tenant_event_categories (tenant_id, name, source) VALUES ($1, $2, $3)
             ON CONFLICT (tenant_id, name) DO NOTHING",
        )
        .bind(tenant_id)
        .bind(name)
        .bind(source)
        .execute(&pool)
        .await
    });
    if let Err(e) = res {
        log::error!("workflows::repo::create_category failed: {e}");
    }
}

/// The tenant's registered events, ordered by category then name. Empty on error.
pub fn list_events(db: &Db, tenant_id: Uuid) -> Vec<TenantEvent> {
    let pool = db.pool();
    db.block(async move {
        sqlx::query_as::<_, (String, String, String, Option<String>, Option<i64>, serde_json::Value)>(
            "SELECT name, category, source, description, created_by_graph, payload
             FROM tenant_events WHERE tenant_id = $1 ORDER BY category, name",
        )
        .bind(tenant_id)
        .fetch_all(&pool)
        .await
        .unwrap_or_else(|e| {
            log::error!("workflows::repo::list_events query failed: {e}");
            Vec::new()
        })
        .into_iter()
        .map(|(name, category, source, description, gid, payload)| TenantEvent {
            name,
            category,
            source,
            description,
            created_by_graph: gid.map(|v| v as u32),
            fields: serde_json::from_value(payload).unwrap_or_default(),
        })
        .collect()
    })
}

/// Register an event (idempotent on name); also ensures its category row exists.
/// Best-effort.
pub fn create_event(
    db: &Db,
    tenant_id: Uuid,
    name: &str,
    category: &str,
    description: Option<&str>,
    created_by_graph: Option<u32>,
    fields: &[EventField],
) {
    let pool = db.pool();
    let name = name.to_string();
    let category = category.to_string();
    let description = description.map(|s| s.to_string());
    let payload = serde_json::to_value(fields).unwrap_or_else(|_| serde_json::json!([]));
    let res: Result<(), sqlx::Error> = db.block(async move {
        let mut tx = pool.begin().await?;
        sqlx::query(
            "INSERT INTO tenant_event_categories (tenant_id, name, source) VALUES ($1, $2, 'user')
             ON CONFLICT (tenant_id, name) DO NOTHING",
        )
        .bind(tenant_id)
        .bind(&category)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO tenant_events (tenant_id, name, category, source, description, created_by_graph, payload)
             VALUES ($1, $2, $3, 'user', $4, $5, $6)
             ON CONFLICT (tenant_id, name) DO UPDATE SET category = EXCLUDED.category, description = EXCLUDED.description, payload = EXCLUDED.payload",
        )
        .bind(tenant_id)
        .bind(&name)
        .bind(&category)
        .bind(&description)
        .bind(created_by_graph.map(|v| v as i64))
        .bind(payload)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    });
    if let Err(e) = res {
        log::error!("workflows::repo::create_event failed: {e}");
    }
}

/// Remove a registered event. Best-effort.
pub fn delete_event(db: &Db, tenant_id: Uuid, name: &str) {
    let pool = db.pool();
    let name = name.to_string();
    let res: Result<_, sqlx::Error> = db.block(async move {
        sqlx::query("DELETE FROM tenant_events WHERE tenant_id = $1 AND name = $2")
            .bind(tenant_id)
            .bind(name)
            .execute(&pool)
            .await
    });
    if let Err(e) = res {
        log::error!("workflows::repo::delete_event failed: {e}");
    }
}

// ---- run history + per-run logs --------------------------------------------

/// A persisted workflow run. Event-triggered runs carry the source graph + event.
#[derive(Debug, Clone, serde::Serialize)]
pub struct RunRecord {
    pub id: Uuid,
    pub graph_id: u32,
    pub graph_name: String,
    pub trigger_source: String,
    pub source_graph_id: Option<u32>,
    pub source_graph_name: Option<String>,
    pub event_name: Option<String>,
    pub pod_name: String,
    pub status: String,
    pub result_summary: Option<String>,
    pub started_at: String,
    pub ended_at: Option<String>,
}

/// One buffered signal line for a run (node activity, log, or event dispatch/trigger/skip).
#[derive(Debug, Clone, serde::Serialize)]
pub struct RunLogLine {
    pub node_id: Option<u32>,
    pub label: Option<String>,
    pub kind: String,
    pub message: String,
}

/// Insert a freshly-started run (`status='running'`). Best-effort.
#[allow(clippy::too_many_arguments)]
pub fn insert_run(
    db: &Db,
    tenant_id: Uuid,
    id: Uuid,
    graph_id: u32,
    graph_name: &str,
    trigger: TriggerSource,
    source_graph_id: Option<u32>,
    source_graph_name: Option<&str>,
    event_name: Option<&str>,
    pod_name: &str,
) {
    let pool = db.pool();
    let graph_name = graph_name.to_string();
    let source_graph_name = source_graph_name.map(|s| s.to_string());
    let event_name = event_name.map(|s| s.to_string());
    let pod_name = pod_name.to_string();
    let res: Result<_, sqlx::Error> = db.block(async move {
        sqlx::query(
            "INSERT INTO workflow_runs
                (id, tenant_id, graph_id, graph_name, trigger_source,
                 source_graph_id, source_graph_name, event_name, pod_name, status, started_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, 'running', $10)",
        )
        .bind(id)
        .bind(tenant_id)
        .bind(graph_id as i64)
        .bind(graph_name)
        .bind(trigger.as_str())
        .bind(source_graph_id.map(|v| v as i64))
        .bind(source_graph_name)
        .bind(event_name)
        .bind(pod_name)
        .bind(crate::alerts::now_rfc3339())
        .execute(&pool)
        .await
    });
    if let Err(e) = res {
        log::error!("workflows::repo::insert_run failed: {e}");
    }
}

/// Mark a run finished with its terminal status + summary. Best-effort.
pub fn finish_run(db: &Db, run_id: Uuid, status: RunStatus, summary: &str) {
    let pool = db.pool();
    let summary = summary.to_string();
    let res: Result<_, sqlx::Error> = db.block(async move {
        sqlx::query(
            "UPDATE workflow_runs SET status = $2, result_summary = $3, ended_at = $4 WHERE id = $1",
        )
        .bind(run_id)
        .bind(status.as_str())
        .bind(summary)
        .bind(crate::alerts::now_rfc3339())
        .execute(&pool)
        .await
    });
    if let Err(e) = res {
        log::error!("workflows::repo::finish_run failed: {e}");
    }
}

/// Batch-insert buffered signal lines for a run. Best-effort.
pub fn insert_run_logs(db: &Db, run_id: Uuid, batch: &[RunLogLine]) {
    if batch.is_empty() {
        return;
    }
    let pool = db.pool();
    let batch = batch.to_vec();
    let res: Result<(), sqlx::Error> = db.block(async move {
        let mut qb = sqlx::QueryBuilder::new(
            "INSERT INTO workflow_run_logs (run_id, node_id, label, kind, message) ",
        );
        qb.push_values(batch.iter(), |mut b, line| {
            b.push_bind(run_id)
                .push_bind(line.node_id.map(|v| v as i64))
                .push_bind(line.label.clone())
                .push_bind(line.kind.clone())
                .push_bind(line.message.clone());
        });
        qb.build().execute(&pool).await?;
        Ok(())
    });
    if let Err(e) = res {
        log::error!("workflows::repo::insert_run_logs failed: {e}");
    }
}

/// Recent runs for a tenant, newest first; optionally filtered to one graph. Empty on error.
pub fn list_runs(db: &Db, tenant_id: Uuid, graph_id: Option<u32>, limit: i64) -> Vec<RunRecord> {
    let pool = db.pool();
    db.block(async move {
        let rows = if let Some(gid) = graph_id {
            sqlx::query_as::<_, RunRow>(
                "SELECT id, graph_id, graph_name, trigger_source, source_graph_id, source_graph_name,
                        event_name, pod_name, status, result_summary, started_at, ended_at
                 FROM workflow_runs WHERE tenant_id = $1 AND graph_id = $2
                 ORDER BY started_at DESC LIMIT $3",
            )
            .bind(tenant_id)
            .bind(gid as i64)
            .bind(limit)
            .fetch_all(&pool)
            .await
        } else {
            sqlx::query_as::<_, RunRow>(
                "SELECT id, graph_id, graph_name, trigger_source, source_graph_id, source_graph_name,
                        event_name, pod_name, status, result_summary, started_at, ended_at
                 FROM workflow_runs WHERE tenant_id = $1
                 ORDER BY started_at DESC LIMIT $2",
            )
            .bind(tenant_id)
            .bind(limit)
            .fetch_all(&pool)
            .await
        };
        rows.unwrap_or_else(|e| {
            log::error!("workflows::repo::list_runs query failed: {e}");
            Vec::new()
        })
        .into_iter()
        .map(RunRow::into_record)
        .collect()
    })
}

/// All signal lines for one run, oldest first. Empty on error or if the run isn't this tenant's.
pub fn list_run_logs(db: &Db, tenant_id: Uuid, run_id: Uuid) -> Vec<RunLogLine> {
    let pool = db.pool();
    db.block(async move {
        sqlx::query_as::<_, (Option<i64>, Option<String>, String, String)>(
            "SELECT l.node_id, l.label, l.kind, l.message
             FROM workflow_run_logs l JOIN workflow_runs r ON r.id = l.run_id
             WHERE l.run_id = $1 AND r.tenant_id = $2 ORDER BY l.id",
        )
        .bind(run_id)
        .bind(tenant_id)
        .fetch_all(&pool)
        .await
        .unwrap_or_else(|e| {
            log::error!("workflows::repo::list_run_logs query failed: {e}");
            Vec::new()
        })
        .into_iter()
        .map(|(node_id, label, kind, message)| RunLogLine {
            node_id: node_id.map(|v| v as u32),
            label,
            kind,
            message,
        })
        .collect()
    })
}

/// On boot, reconcile this pod's runs left `running` by a crash (runs die with the
/// process). Scoped to `pod_name` so a sibling pod's live runs aren't clobbered.
pub fn mark_orphaned_runs(db: &Db, pod_name: &str) {
    let pool = db.pool();
    let pod_name = pod_name.to_string();
    let res: Result<_, sqlx::Error> = db.block(async move {
        sqlx::query(
            "UPDATE workflow_runs SET status = 'failed', result_summary = 'interrupted (pod restart)', ended_at = $2
             WHERE status = 'running' AND pod_name = $1",
        )
        .bind(pod_name)
        .bind(crate::alerts::now_rfc3339())
        .execute(&pool)
        .await
    });
    if let Err(e) = res {
        log::error!("workflows::repo::mark_orphaned_runs failed: {e}");
    }
}

/// Row shape for `workflow_runs` reads; converted to the public [`RunRecord`].
#[derive(sqlx::FromRow)]
struct RunRow {
    id: Uuid,
    graph_id: i64,
    graph_name: String,
    trigger_source: String,
    source_graph_id: Option<i64>,
    source_graph_name: Option<String>,
    event_name: Option<String>,
    pod_name: String,
    status: String,
    result_summary: Option<String>,
    started_at: String,
    ended_at: Option<String>,
}

impl RunRow {
    fn into_record(self) -> RunRecord {
        RunRecord {
            id: self.id,
            graph_id: self.graph_id as u32,
            graph_name: self.graph_name,
            trigger_source: self.trigger_source,
            source_graph_id: self.source_graph_id.map(|v| v as u32),
            source_graph_name: self.source_graph_name,
            event_name: self.event_name,
            pod_name: self.pod_name,
            status: self.status,
            result_summary: self.result_summary,
            started_at: self.started_at,
            ended_at: self.ended_at,
        }
    }
}
