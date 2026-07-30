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
                        log::error!(
                            "workflows::repo::load_all: skipping a malformed graph row: {e}"
                        );
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

/// Every **global** template across all tenants (card #102) — graphs a master flagged
/// `global`, visible to the whole platform. A deliberate cross-tenant read: templates are meant
/// to be shared. Returns `(owner_tenant, graph)`; the owner is kept only so "use" can find the
/// source, never surfaced to other tenants.
pub fn load_global_templates(db: &Db) -> Vec<(Uuid, Graph)> {
    let pool = db.pool();
    db.block(async move {
        let rows = sqlx::query_as::<_, (Uuid, serde_json::Value)>(
            "SELECT tenant_id, data FROM workflows
              WHERE (data->>'global')::boolean IS TRUE AND (data->>'template')::boolean IS TRUE",
        )
        .fetch_all(&pool)
        .await;
        match rows {
            Ok(rows) => rows
                .into_iter()
                .filter_map(|(tid, v)| serde_json::from_value::<Graph>(v).ok().map(|g| (tid, g)))
                .collect(),
            Err(e) => {
                log::error!("workflows::repo::load_global_templates failed: {e}");
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
        sqlx::query("DELETE FROM workflows WHERE tenant_id = $1")
            .bind(tenant_id)
            .execute(&mut *tx)
            .await?;
        for g in &graphs {
            let data = serde_json::to_value(g).unwrap_or_else(|_| serde_json::json!({}));
            sqlx::query("INSERT INTO workflows (tenant_id, id, data) VALUES ($1, $2, $3)")
                .bind(tenant_id)
                .bind(g.id)
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
    // Enabling/disabling a graph changes who listens; drop the cached answer so the next event
    // is judged against the graphs the user just saved, not the ones from up to a TTL ago.
    invalidate_listened_events(tenant_id);
}

// ---- engine state (Phase 0.5) ----------------------------------------------

/// The persisted new-alert high-water mark, or `None` if it was never written
/// (first boot) or on a query error. The adapter falls back to the current alert
/// max when this is `None` so an existing backlog still doesn't fire on first run.
pub fn load_alert_seen_max(db: &Db, tenant_id: Uuid) -> Option<u64> {
    let pool = db.pool();
    db.block(async move {
        sqlx::query_as::<_, (i64,)>(
            "SELECT alert_seen_max FROM workflow_engine_state WHERE tenant_id = $1",
        )
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
pub fn load_cron_state(db: &Db, tenant_id: Uuid) -> Vec<(Uuid, u64)> {
    let pool = db.pool();
    db.block(async move {
        sqlx::query_as::<_, (Uuid, i64)>(
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
        .map(|(id, ms)| (id, ms as u64))
        .collect()
    })
}

/// Record that the tenant's cron graph `graph_id` fired at `fired_ms`. Best-effort.
pub fn save_cron_fired(db: &Db, tenant_id: Uuid, graph_id: Uuid, fired_ms: u64) {
    let pool = db.pool();
    let res: Result<_, sqlx::Error> = db.block(async move {
        sqlx::query(
            "INSERT INTO workflow_cron_state (tenant_id, graph_id, last_fired_ms) VALUES ($1, $2, $3)
             ON CONFLICT (tenant_id, graph_id) DO UPDATE SET last_fired_ms = EXCLUDED.last_fired_ms",
        )
        .bind(tenant_id)
        .bind(graph_id)
        .bind(fired_ms as i64)
        .execute(&pool)
        .await
    });
    if let Err(e) = res {
        log::error!("workflows::repo::save_cron_fired failed: {e}");
    }
}

/// Returns the persisted JSON state for a node, or `None` if never set.
pub fn get_node_state(
    db: &Db,
    tenant_id: Uuid,
    graph_id: Uuid,
    node_id: u32,
    // Which instance of the graph this state belongs to ("" = the single instance).
    instance_key: &str,
) -> Option<serde_json::Value> {
    let instance_key = instance_key.to_string();
    let pool = db.pool();
    db.block(async move {
        sqlx::query_as::<_, (String,)>(
            "SELECT state_json FROM workflow_node_state
              WHERE tenant_id = $1 AND graph_id = $2 AND node_id = $3 AND instance_key = $4",
        )
        .bind(tenant_id)
        .bind(graph_id)
        .bind(node_id as i64)
        .bind(instance_key)
        .fetch_optional(&pool)
        .await
        .ok()
        .flatten()
        .and_then(|(s,)| serde_json::from_str(&s).ok())
    })
}

/// Deletes a node's persisted state, so an idle node leaves no stale row.
pub fn clear_node_state(db: &Db, tenant_id: Uuid, graph_id: Uuid, node_id: u32) {
    let pool = db.pool();
    let res: Result<_, sqlx::Error> = db.block(async move {
        sqlx::query(
            "DELETE FROM workflow_node_state WHERE tenant_id = $1 AND graph_id = $2 AND node_id = $3",
        )
        .bind(tenant_id)
        .bind(graph_id)
        .bind(node_id as i64)
        .execute(&pool)
        .await
    });
    if let Err(e) = res {
        log::error!("workflows::repo::clear_node_state failed: {e}");
    }
}

/// Arm a node's window, but only if it isn't armed already — the leading edge of a debounce.
///
/// Returns `true` if THIS caller armed it (and so owns firing `start`), `false` if it was
/// already armed (a re-trigger, which should only extend). One conditional upsert, so two
/// concurrent runs can't both think they opened the window: exactly one gets `true`.
/// `expires_at` is RFC3339.
pub fn try_arm_node_state(
    db: &Db,
    tenant_id: Uuid,
    graph_id: Uuid,
    node_id: u32,
    // Which instance of the graph this state belongs to ("" = the single instance).
    instance_key: &str,
    expires_at: &str,
) -> bool {
    let pool = db.pool();
    let instance_key = instance_key.to_string();
    let json_str = serde_json::json!({ "state": "armed", "expires_at": expires_at }).to_string();
    let res: Result<u64, sqlx::Error> = db.block(async move {
        let r = sqlx::query(
            "INSERT INTO workflow_node_state (tenant_id, graph_id, node_id, instance_key, state_json)
             VALUES ($1, $2, $3, $5, $4)
             ON CONFLICT (tenant_id, graph_id, node_id, instance_key) DO UPDATE SET state_json = EXCLUDED.state_json
             WHERE (workflow_node_state.state_json::jsonb ->> 'state') IS DISTINCT FROM 'armed'",
        )
        .bind(tenant_id)
        .bind(graph_id)
        .bind(node_id as i64)
        .bind(json_str)
        .bind(instance_key)
        .execute(&pool)
        .await?;
        Ok(r.rows_affected())
    });
    match res {
        Ok(n) => n == 1,
        Err(e) => {
            log::error!("workflows::repo::try_arm_node_state failed: {e}");
            false
        }
    }
}

/// Push an armed node's expiry out to `expires_at`. No-op if it isn't armed any more (the
/// window closed between the read and this write). Returns `true` if it extended.
pub fn extend_node_state(
    db: &Db,
    tenant_id: Uuid,
    graph_id: Uuid,
    node_id: u32,
    // Which instance of the graph this state belongs to ("" = the single instance).
    instance_key: &str,
    expires_at: &str,
) -> bool {
    let pool = db.pool();
    let json_str = serde_json::json!({ "state": "armed", "expires_at": expires_at }).to_string();
    let instance_key = instance_key.to_string();
    let res: Result<u64, sqlx::Error> = db.block(async move {
        let r = sqlx::query(
            // instance_key MUST be in the WHERE: without it a re-trigger on one instance extends
            // every armed instance's window for this node (the UPDATE has no LIMIT). Harmless under
            // Single scope — one row, key "" — but wrong the moment a graph is PerCamera.
            "UPDATE workflow_node_state SET state_json = $4
             WHERE tenant_id = $1 AND graph_id = $2 AND node_id = $3 AND instance_key = $5
               AND (state_json::jsonb ->> 'state') = 'armed'",
        )
        .bind(tenant_id)
        .bind(graph_id)
        .bind(node_id as i64)
        .bind(json_str)
        .bind(instance_key)
        .execute(&pool)
        .await?;
        Ok(r.rows_affected())
    });
    match res {
        Ok(n) => n == 1,
        Err(e) => {
            log::error!("workflows::repo::extend_node_state failed: {e}");
            false
        }
    }
}

/// Close an armed window whose expiry has passed — the trailing edge of a debounce.
///
/// Returns `true` only if THIS caller closed it (and so owns firing `stop`). Returns `false`
/// when a re-trigger extended `expires_at` into the future, or the window is already closed.
/// Postgres compares the expiry against its own `now()`, so the decision is atomic with the
/// write and immune to clock skew between pods.
pub fn try_disarm_expired_node_state(
    db: &Db,
    tenant_id: Uuid,
    graph_id: Uuid,
    node_id: u32,
    // Which instance of the graph this state belongs to ("" = the single instance).
    instance_key: &str,
) -> bool {
    let pool = db.pool();
    let instance_key = instance_key.to_string();
    let res: Result<u64, sqlx::Error> = db.block(async move {
        let r = sqlx::query(
            "UPDATE workflow_node_state SET state_json = '{\"state\":\"idle\"}'
             WHERE tenant_id = $1 AND graph_id = $2 AND node_id = $3 AND instance_key = $4
               AND (state_json::jsonb ->> 'state') = 'armed'
               AND (state_json::jsonb ->> 'expires_at')::timestamptz <= now()",
        )
        .bind(tenant_id)
        .bind(graph_id)
        .bind(node_id as i64)
        .bind(instance_key)
        .execute(&pool)
        .await?;
        Ok(r.rows_affected())
    });
    match res {
        Ok(n) => n == 1,
        Err(e) => {
            log::error!("workflows::repo::try_disarm_expired_node_state failed: {e}");
            false
        }
    }
}

/// Upserts the JSON state for a node. Best-effort (errors are logged, not propagated).
pub fn set_node_state(
    db: &Db,
    tenant_id: Uuid,
    graph_id: Uuid,
    node_id: u32,
    // Which instance of the graph this state belongs to ("" = the single instance). Must be part
    // of the ON CONFLICT target, or it no longer matches the PK (0057 widened it) and every write
    // fails — which is exactly what Cooldown/FlipFlop/TimeGate hit after phase 1.
    instance_key: &str,
    state: &serde_json::Value,
) {
    let pool = db.pool();
    let json_str = state.to_string();
    let instance_key = instance_key.to_string();
    let res: Result<_, sqlx::Error> = db.block(async move {
        sqlx::query(
            "INSERT INTO workflow_node_state (tenant_id, graph_id, node_id, instance_key, state_json) VALUES ($1, $2, $3, $5, $4)
             ON CONFLICT (tenant_id, graph_id, node_id, instance_key) DO UPDATE SET state_json = EXCLUDED.state_json",
        )
        .bind(tenant_id)
        .bind(graph_id)
        .bind(node_id as i64)
        .bind(json_str)
        .bind(instance_key)
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
    pub created_by_graph: Option<Uuid>,
    /// The typed payload schema (possibly empty).
    pub fields: Vec<EventField>,
}

/// Platform-defined events available to EVERY tenant, with their payload schema. Merged into
/// the event list the editor fetches, so `OnEvent` nodes expose typed outputs immediately —
/// no per-tenant registration and no first-event chicken-and-egg. System events are emitted
/// by the platform (e.g. `motion` by the edge), not user `EmitEvent` nodes. Add new ones here.
pub fn system_events() -> Vec<TenantEvent> {
    let field = |name: &str, ty: PortType| EventField {
        name: name.to_string(),
        ty,
    };
    vec![
        TenantEvent {
            name: "motion".to_string(),
            category: "System".to_string(),
            source: "system".to_string(),
            description: Some("Edge motion detection".to_string()),
            created_by_graph: None,
            fields: vec![
                field("camera", PortType::Camera),
                field("score", PortType::Num),
                field("area", PortType::Num),
                field("blobs", PortType::Num),
                field("bbox", PortType::Bbox),
                field("frame", PortType::Image),
            ],
        },
        TenantEvent {
            name: "face_detected".to_string(),
            category: "System".to_string(),
            source: "system".to_string(),
            description: Some(
                "Edge face detection (YuNet) — feeds the recognition pipeline".to_string(),
            ),
            created_by_graph: None,
            fields: vec![
                field("camera", PortType::Camera),
                field("bbox", PortType::Bbox),
                field("confidence", PortType::Num), // 0..100
                field("face_width", PortType::Num), // face box width in source pixels
                field("kps", PortType::Landmarks), // 5 YuNet landmarks, source px (#122; empty until edge emits)
                field("frame", PortType::Image),
            ],
        },
        // Camera reachability, as a first-class event so "tell me when a camera drops" is a graph
        // the user builds, not a feature we have to anticipate. Emitted on TRANSITION only — a
        // camera that is offline for a day produces ONE `camera_offline`, not one per probe, or a
        // flapping camera would bury a tenant's feed and its notification quota.
        //
        // `reason` is `rtsp_probe::ProbeStatus` — unreachable | timeout | auth_failed | locked |
        // not_found | error — so a graph can branch on the CAUSE, not just the fact. "Power-cycle
        // it" is right for `locked` and wrong for `auth_failed`, and only the reason distinguishes
        // them.
        TenantEvent {
            name: "camera_offline".to_string(),
            category: "System".to_string(),
            source: "system".to_string(),
            description: Some(
                "A camera stopped responding — the edge can no longer reach or read it".to_string(),
            ),
            created_by_graph: None,
            fields: vec![
                field("camera", PortType::Camera),
                field("reason", PortType::Text),
                field("detail", PortType::Text),
            ],
        },
        TenantEvent {
            name: "camera_online".to_string(),
            category: "System".to_string(),
            source: "system".to_string(),
            description: Some(
                "A camera started responding again after being offline".to_string(),
            ),
            created_by_graph: None,
            fields: vec![
                field("camera", PortType::Camera),
                // How long it was down, so a graph can stay quiet about a 20-second blip and speak
                // up about an hour — without every user having to build that logic themselves.
                field("offline_secs", PortType::Num),
            ],
        },
        TenantEvent {
            name: "edge:full-disk".to_string(),
            category: "System".to_string(),
            source: "system".to_string(),
            description: Some(
                "Edge recording storage is near or over the configured limit".to_string(),
            ),
            created_by_graph: None,
            fields: vec![
                field("used_bytes", PortType::Num),
                field("max_bytes", PortType::Num),
                field("pct", PortType::Num),
                field("path", PortType::Text),
            ],
        },
        // A paired Telegram user sent a message. The notification service enqueues this from its
        // long-poll loop (only for chat_ids paired in THIS tenant — see notification::handler),
        // so a graph's `On event("telegram_message")` becomes the bot brain. `command`/`args` are
        // pre-split at the source (`command` = a leading "/word", else empty; `args` = the rest).
        TenantEvent {
            name: "telegram_message".to_string(),
            category: "System".to_string(),
            source: "system".to_string(),
            description: Some("A paired Telegram user sent a message".to_string()),
            created_by_graph: None,
            fields: vec![
                field("text", PortType::Text),
                field("chat_id", PortType::Num),
                field("from_name", PortType::Text),
                field("command", PortType::Text),
                field("args", PortType::Text),
            ],
        },
        // The edge object tracker started tracking a new entity in a camera's view (card: edge
        // tracking). `uuid` is the stable per-entity id (same across graphs + the map); `count` is
        // how many of `class` are currently in view. `bbox` is `[0,0,0,0]` (meaningless) when
        // `zone` is set — the node has a region-of-interest drawn (property panel), so this is a
        // ZONE entry, not a whole-frame one; `zone` is empty/absent for whole-frame entries.
        TenantEvent {
            name: "object_entered".to_string(),
            category: "System".to_string(),
            source: "system".to_string(),
            description: Some("The tracker started tracking a new object in view".to_string()),
            created_by_graph: None,
            fields: vec![
                field("uuid", PortType::Text),
                field("class", PortType::Text),
                field("camera", PortType::Camera),
                field("bbox", PortType::Bbox),
                field("count", PortType::Num),
                field("zone", PortType::Text),
            ],
        },
        // A tracked entity left the camera's view (dropped after the tracker's lost-buffer window)
        // — or left the configured zone, if the node has one drawn (see `object_entered`).
        // `dwell_secs` is how long it was on screen (or in the zone).
        TenantEvent {
            name: "object_exited".to_string(),
            category: "System".to_string(),
            source: "system".to_string(),
            description: Some("A tracked object left the camera's view".to_string()),
            created_by_graph: None,
            fields: vec![
                field("uuid", PortType::Text),
                field("class", PortType::Text),
                field("camera", PortType::Camera),
                field("dwell_secs", PortType::Num),
                field("count", PortType::Num),
                field("zone", PortType::Text),
            ],
        },
        // A tracked entity's centre crossed a configured virtual line (card: edge tracking, P3).
        // `line` is the crossed line's name; `direction` is "forward"/"backward" relative to the
        // line's own p1→p2 direction.
        TenantEvent {
            name: "line_crossed".to_string(),
            category: "System".to_string(),
            source: "system".to_string(),
            description: Some("A tracked object crossed a configured virtual line".to_string()),
            created_by_graph: None,
            fields: vec![
                field("uuid", PortType::Text),
                field("class", PortType::Text),
                field("camera", PortType::Camera),
                field("line", PortType::Text),
                field("direction", PortType::Text),
                field("count", PortType::Num),
            ],
        },
    ]
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
        sqlx::query_as::<
            _,
            (
                String,
                String,
                String,
                Option<String>,
                Option<Uuid>,
                serde_json::Value,
            ),
        >(
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
        .map(
            |(name, category, source, description, gid, payload)| TenantEvent {
                name,
                category,
                source,
                description,
                created_by_graph: gid,
                fields: serde_json::from_value(payload).unwrap_or_default(),
            },
        )
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
    created_by_graph: Option<Uuid>,
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
        .bind(created_by_graph)
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
    pub graph_id: Uuid,
    pub graph_name: String,
    pub trigger_source: String,
    pub source_graph_id: Option<Uuid>,
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
    /// "node" (UI animation) or "log" (execution result).
    pub kind: String,
    /// The node's kind slug (e.g. "person_detection"), used for UI colouring.
    pub node_kind: Option<String>,
    pub message: String,
}

/// Insert a freshly-started run (`status='running'`). Best-effort.
#[allow(clippy::too_many_arguments)]
pub fn insert_run(
    db: &Db,
    tenant_id: Uuid,
    id: Uuid,
    graph_id: Uuid,
    graph_name: &str,
    trigger: TriggerSource,
    source_graph_id: Option<Uuid>,
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
        .bind(graph_id)
        .bind(graph_name)
        .bind(trigger.as_str())
        .bind(source_graph_id)
        .bind(source_graph_name)
        .bind(event_name)
        .bind(pod_name)
        .bind(crate::timeutil::now_rfc3339())
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
        .bind(crate::timeutil::now_rfc3339())
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
pub fn list_runs(db: &Db, tenant_id: Uuid, graph_id: Option<Uuid>, limit: i64) -> Vec<RunRecord> {
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
            .bind(gid)
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

/// The most recent run summary for a graph — for the list's "last run" column.
#[derive(Clone, serde::Serialize)]
pub struct LastRun {
    pub at: String,      // RFC3339 started_at
    pub status: String,  // "running" | "ok" | "failed"
    pub trigger: String, // "manual" | "cron" | "event"
}

/// The latest run per graph for the tenant, keyed by graph id. One batch query
/// (DISTINCT ON) so the list doesn't fan out a query per row. Empty on error.
pub fn last_run_by_graph(db: &Db, tenant_id: Uuid) -> std::collections::HashMap<Uuid, LastRun> {
    let pool = db.pool();
    db.block(async move {
        sqlx::query_as::<_, (Uuid, String, String, String)>(
            "SELECT DISTINCT ON (graph_id) graph_id, started_at, status, trigger_source
             FROM workflow_runs WHERE tenant_id = $1
             ORDER BY graph_id, started_at DESC",
        )
        .bind(tenant_id)
        .fetch_all(&pool)
        .await
        .unwrap_or_else(|e| {
            log::error!("workflows::repo::last_run_by_graph query failed: {e}");
            Vec::new()
        })
        .into_iter()
        .map(|(gid, at, status, trigger)| {
            (
                gid,
                LastRun {
                    at,
                    status,
                    trigger,
                },
            )
        })
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
            node_kind: None, // not persisted in this table; only carried in-memory for platform_logs
            message,
        })
        .collect()
    })
}

/// One recent execution line for a graph's editor console. Sourced from the LIVE `workflow_run_logs`
/// (card #145: workflow node logs stay in-app — they are product data, not the debug firehose that
/// moved to Datadog — so the editor console + per-run panel read this table, NOT the frozen
/// `platform_logs` mirror). `at` is the run's start time (run-logs have no per-line timestamp; all
/// lines of a run share the run time).
#[derive(serde::Serialize)]
pub struct ConsoleLogLine {
    pub label: Option<String>,
    pub kind: String,
    pub message: String,
    pub at: String,
}

/// Recent execution lines across a graph's runs, newest first (bounded, `node`-kind UI pings
/// excluded). Powers the workflow editor's console seed.
pub fn list_graph_run_logs(
    db: &Db,
    tenant_id: Uuid,
    graph_id: Uuid,
    limit: i64,
) -> Vec<ConsoleLogLine> {
    let pool = db.pool();
    let limit = limit.clamp(1, 200);
    db.block(async move {
        sqlx::query_as::<_, (Option<String>, String, String, String)>(
            "SELECT l.label, l.kind, l.message, r.started_at
             FROM workflow_run_logs l JOIN workflow_runs r ON r.id = l.run_id
             WHERE r.tenant_id = $1 AND r.graph_id = $2 AND l.kind <> 'node'
             ORDER BY l.id DESC LIMIT $3",
        )
        .bind(tenant_id)
        .bind(graph_id)
        .bind(limit)
        .fetch_all(&pool)
        .await
        .unwrap_or_else(|e| {
            log::error!("workflows::repo::list_graph_run_logs query failed: {e}");
            Vec::new()
        })
        .into_iter()
        .map(|(label, kind, message, at)| ConsoleLogLine {
            label,
            kind,
            message,
            at,
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
        .bind(crate::timeutil::now_rfc3339())
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
    graph_id: Uuid,
    graph_name: String,
    trigger_source: String,
    source_graph_id: Option<Uuid>,
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
            graph_id: self.graph_id,
            graph_name: self.graph_name,
            trigger_source: self.trigger_source,
            source_graph_id: self.source_graph_id,
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

// ---- an instance's value context (cards #95 phase 3 / #94) -------------------

/// Read what nodes last produced in this instance.
///
/// Separate column from `state_json` on purpose: the window's writers rewrite that document whole
/// (`try_arm` replaces it, the disarm sets `{"state":"idle"}`), so values kept there would be wiped
/// by the very transitions they must outlive — the window closing is exactly when the stop branch
/// needs them.
pub fn get_node_values(
    db: &Db,
    tenant_id: Uuid,
    graph_id: Uuid,
    node_id: u32,
    instance_key: &str,
) -> Option<serde_json::Value> {
    let pool = db.pool();
    let instance_key = instance_key.to_string();
    let res: Result<Option<(String,)>, sqlx::Error> = db.block(async move {
        sqlx::query_as::<_, (String,)>(
            "SELECT values_json FROM workflow_node_state
              WHERE tenant_id = $1 AND graph_id = $2 AND node_id = $3 AND instance_key = $4",
        )
        .bind(tenant_id)
        .bind(graph_id)
        .bind(node_id as i64)
        .bind(instance_key)
        .fetch_optional(&pool)
        .await
    });
    match res {
        Ok(row) => row.and_then(|(s,)| serde_json::from_str(&s).ok()),
        Err(e) => {
            log::error!("workflows::repo::get_node_values failed: {e}");
            None
        }
    }
}

/// Record what a node produced in this instance. Upsert, touching only `values_json` — a node that
/// didn't run this time keeps what it produced last time, which is what lets a re-trigger (which
/// never re-runs `StartRecording`) leave the arming run's `clip_id` intact.
pub fn set_node_values(
    db: &Db,
    tenant_id: Uuid,
    graph_id: Uuid,
    node_id: u32,
    instance_key: &str,
    values: &serde_json::Value,
) {
    let pool = db.pool();
    let instance_key = instance_key.to_string();
    let json_str = values.to_string();
    let res: Result<u64, sqlx::Error> = db.block(async move {
        let r = sqlx::query(
            "INSERT INTO workflow_node_state (tenant_id, graph_id, node_id, instance_key, values_json)
             VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT (tenant_id, graph_id, node_id, instance_key)
             DO UPDATE SET values_json = EXCLUDED.values_json",
        )
        .bind(tenant_id)
        .bind(graph_id)
        .bind(node_id as i64)
        .bind(instance_key)
        .bind(json_str)
        .execute(&pool)
        .await?;
        Ok(r.rows_affected())
    });
    if let Err(e) = res {
        log::error!("workflows::repo::set_node_values failed: {e}");
    }
}

/// Read the variables snapshot the DelayTrigger stored when it armed this instance's window.
///
/// A SEPARATE column from `state_json`/`values_json` for the same reason as values: the window's
/// writers rewrite state_json whole, so anything kept there is wiped by the very transitions it has
/// to outlive. `sys.variables` is a fresh map per run, so without this the trailing-edge stop run
/// reads a type-zero for anything a `Set` wrote on the arming run — card #114.
pub fn get_node_variables(
    db: &Db,
    tenant_id: Uuid,
    graph_id: Uuid,
    node_id: u32,
    instance_key: &str,
) -> Option<serde_json::Value> {
    let pool = db.pool();
    let instance_key = instance_key.to_string();
    let res: Result<Option<(String,)>, sqlx::Error> = db.block(async move {
        sqlx::query_as::<_, (String,)>(
            "SELECT variables_json FROM workflow_node_state
              WHERE tenant_id = $1 AND graph_id = $2 AND node_id = $3 AND instance_key = $4",
        )
        .bind(tenant_id)
        .bind(graph_id)
        .bind(node_id as i64)
        .bind(instance_key)
        .fetch_optional(&pool)
        .await
    });
    match res {
        Ok(row) => row.and_then(|(s,)| serde_json::from_str(&s).ok()),
        Err(e) => {
            log::error!("workflows::repo::get_node_variables failed: {e}");
            None
        }
    }
}

/// Snapshot this instance's variables onto the DelayTrigger's row. Upsert touching only
/// `variables_json`, keyed by the same (tenant, graph, node, instance_key) as the state and values —
/// so camera A's snapshot can't be read by camera B's stop.
pub fn set_node_variables(
    db: &Db,
    tenant_id: Uuid,
    graph_id: Uuid,
    node_id: u32,
    instance_key: &str,
    variables: &serde_json::Value,
) {
    let pool = db.pool();
    let instance_key = instance_key.to_string();
    let json_str = variables.to_string();
    let res: Result<u64, sqlx::Error> = db.block(async move {
        let r = sqlx::query(
            "INSERT INTO workflow_node_state (tenant_id, graph_id, node_id, instance_key, variables_json)
             VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT (tenant_id, graph_id, node_id, instance_key)
             DO UPDATE SET variables_json = EXCLUDED.variables_json",
        )
        .bind(tenant_id)
        .bind(graph_id)
        .bind(node_id as i64)
        .bind(instance_key)
        .bind(json_str)
        .execute(&pool)
        .await?;
        Ok(r.rows_affected())
    });
    if let Err(e) = res {
        log::error!("workflows::repo::set_node_variables failed: {e}");
    }
}

// ---- live instances (card #95 phase 4) --------------------------------------

/// One live/known instance of a graph.
#[derive(Debug, Clone)]
pub struct InstanceInfo {
    /// The graph's `InstanceScope` applied to the firing signal — `""` for a `Single`-scope graph,
    /// the camera id for `PerCamera`.
    pub instance_key: String,
    /// A latent window is still open in this instance: some node's `state_json` reads `state:armed`
    /// (a `DelayTrigger`/`TimeGate` that armed and hasn't been disarmed). This is what "in flight"
    /// means for the live view.
    pub active: bool,
    /// When the still-open window closes (the max armed `expires_at`, RFC3339). `None` when the
    /// instance has only saved values and no live window. This is the one per-instance timestamp the
    /// row carries — `workflow_node_state` has no generic mutation column (no migration this slice).
    pub window_expires_at: Option<String>,
}

/// Enumerate a graph's instances.
///
/// The honest data source is **distinct `instance_key`s present in `workflow_node_state`** for the
/// (tenant, graph): a row exists exactly when a run of that instance saved latent state (#95 phase 1)
/// or node values (#94 phase 2), so this answers "which run contexts are alive or have saved data" —
/// the question the live view asks. It deliberately does NOT invent an instance from `workflow_runs`,
/// because runs are not yet instance-keyed (that's #95 phase 2, "an instance = an unfinished run
/// row"); attributing a run to an instance here would be a guess. The caller enriches with the
/// graph's latest run at the endpoint, flagged as best-effort.
///
/// `active`/`window_expires_at` are derived in Rust from each row's `state_json` (a small JSON blob
/// whose `expires_at` is RFC3339 text inside the document, not a comparable SQL column), so grouping
/// and the armed check happen here rather than in a jsonb query.
pub fn list_instances(db: &Db, tenant_id: Uuid, graph_id: Uuid) -> Vec<InstanceInfo> {
    let pool = db.pool();
    let rows: Vec<(String, String)> = db.block(async move {
        sqlx::query_as::<_, (String, String)>(
            "SELECT instance_key, state_json FROM workflow_node_state
              WHERE tenant_id = $1 AND graph_id = $2",
        )
        .bind(tenant_id)
        .bind(graph_id)
        .fetch_all(&pool)
        .await
        .unwrap_or_else(|e| {
            log::error!("workflows::repo::list_instances query failed: {e}");
            Vec::new()
        })
    });
    instances_from_state_rows(rows)
}

/// Pure grouping of `(instance_key, state_json)` rows into instances: one entry per distinct key,
/// `active` if any of its nodes' `state_json` reads `state:armed`, `window_expires_at` the latest
/// armed `expires_at`. Split out from [`list_instances`] so it's testable without a DB.
fn instances_from_state_rows(rows: Vec<(String, String)>) -> Vec<InstanceInfo> {
    let mut by_key: std::collections::HashMap<String, InstanceInfo> =
        std::collections::HashMap::new();
    for (key, state_json) in rows {
        let entry = by_key.entry(key.clone()).or_insert(InstanceInfo {
            instance_key: key,
            active: false,
            window_expires_at: None,
        });
        let Ok(state) = serde_json::from_str::<serde_json::Value>(&state_json) else {
            continue;
        };
        if state.get("state").and_then(|v| v.as_str()) != Some("armed") {
            continue;
        }
        entry.active = true;
        if let Some(exp) = state.get("expires_at").and_then(|v| v.as_str()) {
            // Keep the LATEST expiry across the instance's armed nodes (RFC3339 sorts lexically).
            let keep = entry
                .window_expires_at
                .as_deref()
                .map_or(true, |cur| exp > cur);
            if keep {
                entry.window_expires_at = Some(exp.to_string());
            }
        }
    }

    let mut out: Vec<InstanceInfo> = by_key.into_values().collect();
    // Deterministic order: active instances first, then by key.
    out.sort_by(|a, b| {
        b.active
            .cmp(&a.active)
            .then_with(|| a.instance_key.cmp(&b.instance_key))
    });
    out
}

/// Every node's saved value context for one instance: `(node_id, parsed values_json)`, skipping rows
/// with no saved values (`{}`). The caller decodes each blob with `values::from_json` and renders it.
pub fn instance_node_values(
    db: &Db,
    tenant_id: Uuid,
    graph_id: Uuid,
    instance_key: &str,
) -> Vec<(u32, serde_json::Value)> {
    let pool = db.pool();
    let instance_key = instance_key.to_string();
    let rows: Vec<(i64, String)> = db.block(async move {
        sqlx::query_as::<_, (i64, String)>(
            "SELECT node_id, values_json FROM workflow_node_state
              WHERE tenant_id = $1 AND graph_id = $2 AND instance_key = $3
              ORDER BY node_id",
        )
        .bind(tenant_id)
        .bind(graph_id)
        .bind(instance_key)
        .fetch_all(&pool)
        .await
        .unwrap_or_else(|e| {
            log::error!("workflows::repo::instance_node_values query failed: {e}");
            Vec::new()
        })
    });
    rows.into_iter()
        .filter_map(|(nid, s)| {
            let v: serde_json::Value = serde_json::from_str(&s).ok()?;
            match v.as_object() {
                Some(o) if !o.is_empty() => Some((nid as u32, v)),
                _ => None,
            }
        })
        .collect()
}

/// The most recent run of one graph (`id`, `status`, `started_at`), if any. Used to annotate the
/// instance list with the graph's latest run — best-effort, since runs aren't instance-keyed yet.
pub fn latest_run_for_graph(
    db: &Db,
    tenant_id: Uuid,
    graph_id: Uuid,
) -> Option<(Uuid, String, String)> {
    let pool = db.pool();
    db.block(async move {
        sqlx::query_as::<_, (Uuid, String, String)>(
            "SELECT id, status, started_at FROM workflow_runs
              WHERE tenant_id = $1 AND graph_id = $2
              ORDER BY started_at DESC LIMIT 1",
        )
        .bind(tenant_id)
        .bind(graph_id)
        .fetch_optional(&pool)
        .await
        .unwrap_or_else(|e| {
            log::error!("workflows::repo::latest_run_for_graph query failed: {e}");
            None
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instances_group_by_key_and_flag_armed_windows() {
        // Two cameras through one PerCamera graph, each with a DelayTrigger row: cam-A armed
        // (window open), cam-B idle (window closed). A third row for cam-A is a plain saved-value
        // node (no window). The instance list must show BOTH cameras as distinct instances, cam-A
        // active with its expiry, cam-B present but inactive.
        let rows = vec![
            (
                "cam-A".to_string(),
                r#"{"state":"armed","expires_at":"2026-07-18T10:00:00+00:00"}"#.to_string(),
            ),
            ("cam-A".to_string(), "{}".to_string()), // a non-window node's state
            ("cam-B".to_string(), r#"{"state":"idle"}"#.to_string()),
        ];
        let out = instances_from_state_rows(rows);
        assert_eq!(out.len(), 2, "two distinct instances");
        // active first, then by key
        assert_eq!(out[0].instance_key, "cam-A");
        assert!(out[0].active, "cam-A's window is armed");
        assert_eq!(
            out[0].window_expires_at.as_deref(),
            Some("2026-07-18T10:00:00+00:00")
        );
        assert_eq!(out[1].instance_key, "cam-B");
        assert!(!out[1].active, "cam-B's window is idle");
        assert_eq!(out[1].window_expires_at, None);
    }

    #[test]
    fn instances_keep_the_latest_expiry_across_armed_nodes() {
        // One instance, two armed latent nodes with different expiries: the instance reports the
        // later one (the window is open until the last of them closes).
        let rows = vec![
            (
                "".to_string(),
                r#"{"state":"armed","expires_at":"2026-07-18T10:00:00+00:00"}"#.to_string(),
            ),
            (
                "".to_string(),
                r#"{"state":"armed","expires_at":"2026-07-18T10:05:00+00:00"}"#.to_string(),
            ),
        ];
        let out = instances_from_state_rows(rows);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].instance_key, "");
        assert!(out[0].active);
        assert_eq!(
            out[0].window_expires_at.as_deref(),
            Some("2026-07-18T10:05:00+00:00")
        );
    }
}

// ---- event listener set (write-amplification guard) -------------------------
// The edge pushes `motion`/`face_detected` with the triggering FRAME inline. Every push was
// enqueued as a durable job regardless of whether any graph was listening, so a tenant whose
// workflows were all switched off still paid: frame encoded on the box, uploaded, written into
// Postgres, read back by flow, matched against nothing, discarded. One production tenant had
// 39,110 motion jobs totalling 6.5 GB of payload accumulated this way, all of it unreachable.
//
// The fix is to ask, before enqueuing, whether ANY enabled graph handles the event. That answer
// changes only when a workflow is saved or toggled, so it is cached per tenant and invalidated in
// `save()`; the TTL is a backstop for writes that bypass this process (another replica).

use std::collections::HashSet;
use std::time::{Duration, Instant};

/// Backstop for cross-replica edits — `save()` invalidates locally and immediately.
const LISTENERS_TTL: Duration = Duration::from_secs(30);

type ListenerCache = std::collections::HashMap<Uuid, (HashSet<String>, Instant)>;

fn listener_cache() -> &'static std::sync::Mutex<ListenerCache> {
    static CACHE: std::sync::OnceLock<std::sync::Mutex<ListenerCache>> = std::sync::OnceLock::new();
    CACHE.get_or_init(|| std::sync::Mutex::new(ListenerCache::new()))
}

/// Drop a tenant's cached listener set — call whenever its graphs change.
pub fn invalidate_listened_events(tenant_id: Uuid) {
    listener_cache().lock().unwrap().remove(&tenant_id);
}

/// Is the cached entry still usable? Split out so the TTL rule is unit-testable without a DB.
fn cache_entry_fresh(at: Instant, now: Instant, ttl: Duration) -> bool {
    now.duration_since(at) < ttl
}

/// Event names that at least one **enabled** graph of this tenant handles via `OnEvent`.
///
/// `None` means "could not determine" (query error) and callers must deliver anyway — never
/// confuse it with `Some(empty)`, which is the authoritative "this tenant listens for nothing"
/// and is precisely the case worth skipping. Collapsing the two would make the guard a no-op for
/// the tenant that motivated it.
pub fn listened_events(db: &Db, tenant_id: Uuid) -> Option<HashSet<String>> {
    if let Some((set, at)) = listener_cache().lock().unwrap().get(&tenant_id) {
        if cache_entry_fresh(*at, Instant::now(), LISTENERS_TTL) {
            return Some(set.clone());
        }
    }
    let pool = db.pool();
    let rows: Result<Vec<(String,)>, sqlx::Error> = db.block(async move {
        sqlx::query_as(
            "SELECT DISTINCT n->'config'->>'event' \
             FROM workflows w, jsonb_array_elements(w.data->'nodes') n \
             WHERE w.tenant_id = $1 \
               AND (w.data->'enabled')::boolean \
               AND n->>'kind' = 'on_event' \
               AND n->'config'->>'event' IS NOT NULL",
        )
        .bind(tenant_id)
        .fetch_all(&pool)
        .await
    });
    let set: HashSet<String> = match rows {
        Ok(r) => r.into_iter().map(|(e,)| e).collect(),
        Err(e) => {
            // Fail OPEN: a query error must not silently stop delivering events.
            log::warn!("listened_events({tenant_id}) failed, allowing the event through: {e}");
            return None;
        }
    };
    listener_cache()
        .lock()
        .unwrap()
        .insert(tenant_id, (set.clone(), Instant::now()));
    Some(set)
}

#[cfg(test)]
mod listener_cache_tests {
    use super::*;

    #[test]
    fn a_fresh_entry_is_reused_and_a_stale_one_is_not() {
        let now = Instant::now();
        let ttl = Duration::from_secs(30);
        assert!(
            cache_entry_fresh(now, now, ttl),
            "just-written entry is fresh"
        );
        assert!(
            cache_entry_fresh(now, now + Duration::from_secs(29), ttl),
            "inside the TTL it is still fresh"
        );
        assert!(
            !cache_entry_fresh(now, now + Duration::from_secs(31), ttl),
            "past the TTL it must be re-read, so a graph enabled on another replica takes effect"
        );
    }

    #[test]
    fn invalidate_forces_a_re_read() {
        let t = Uuid::new_v4();
        listener_cache()
            .lock()
            .unwrap()
            .insert(t, (HashSet::from(["motion".to_string()]), Instant::now()));
        assert!(listener_cache().lock().unwrap().contains_key(&t));
        invalidate_listened_events(t);
        assert!(
            !listener_cache().lock().unwrap().contains_key(&t),
            "toggling a workflow must take effect at once, not after the TTL"
        );
    }
}
