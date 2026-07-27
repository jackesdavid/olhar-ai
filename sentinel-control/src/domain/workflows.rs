//! Tenant-scoped workflow graph editor: CRUD on the graph collection (persisted to
//! the shared Postgres, where `sentinel-flow` reads it), plus run / run-state which
//! proxy to the always-on `sentinel-flow` engine. Rust stays authoritative for ids
//! and typed-edge validation — each edit loads the tenant's graphs, mutates the
//! in-memory `Workflows`, and saves replace-all, then pokes flow to re-read.
use std::convert::Infallible;
use std::time::Duration;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Json};
use axum::routing::{get, post, put};
use axum::Router;
use futures::StreamExt;
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use app_core::workflows::{
    self, ConcurrencyPolicy, Graph as WfGraph, InstanceScope, NodeKind as WfNodeKind, Schedule,
    Trigger, VariableDecl, Viewport, Workflows,
};

use super::{blocking, err, tenant_of, ApiErr};
use crate::auth::{self, UserClaims};
use crate::{store, AppState};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/workflows", get(list).post(create))
        .route("/workflows/catalog", get(catalog))
        .route("/workflows/runs", get(run_states))
        .route("/workflows/runs/history", get(run_history))
        .route("/workflows/runs/frame", get(run_frame))
        .route("/workflows/runs/node-state", get(node_state))
        .route("/workflows/runs/:run_id/logs", get(run_logs))
        .route("/workflows/:id/console-logs", get(console_logs))
        .route("/workflows/events", get(events_stream))
        .route(
            "/workflows/event-registry",
            get(list_events).post(create_event),
        )
        .route(
            "/workflows/event-registry/:name",
            axum::routing::delete(delete_event),
        )
        .route(
            "/workflows/event-categories",
            get(list_categories).post(create_category),
        )
        .route(
            "/workflows/:id",
            axum::routing::patch(rename).delete(remove),
        )
        .route("/workflows/:id/instances", get(instances))
        .route("/workflows/:id/instance-values", get(instance_values))
        .route("/workflows/:id/description", put(set_description))
        .route("/workflows/:id/group", put(set_group))
        .route("/workflows/:id/duplicate", post(duplicate))
        .route("/workflows/templates", get(list_templates))
        .route("/workflows/templates/:id/use", post(use_template))
        .route("/workflows/:id/template", put(set_template))
        .route("/workflows/:id/global", put(set_global))
        .route("/workflows/:id/trigger", put(set_trigger))
        .route("/workflows/:id/concurrency", put(set_concurrency))
        .route("/workflows/:id/instance-scope", put(set_instance_scope))
        .route("/workflows/:id/viewport", put(set_viewport))
        .route("/workflows/:id/edge-vias", put(set_edge_vias))
        .route("/workflows/:id/variables", put(set_variables))
        .route("/workflows/:id/enabled", put(set_enabled))
        .route("/workflows/:id/run-orphans", put(set_run_orphans))
        .route("/workflows/:id/debug", put(set_debug))
        .route("/workflows/:id/run", post(run))
        .route("/workflows/:id/graph", put(save_graph))
        .route("/workflows/:id/nodes", post(add_node))
        .route(
            "/workflows/:id/nodes/:nid",
            axum::routing::delete(remove_node),
        )
        .route("/workflows/:id/nodes/:nid/move", put(move_node))
        .route("/workflows/:id/nodes/:nid/config", put(set_node_config))
        .route("/workflows/:id/nodes/:nid/memoize", put(set_node_memoize))
        .route("/workflows/:id/edges", post(add_edge))
        .route(
            "/workflows/:id/edges/:idx",
            axum::routing::delete(remove_edge),
        )
}

// ---- sentinel-flow proxy ----------------------------------------------------

/// The workflow engine service base URL (`FLOW_URL`, default `127.0.0.1:8787`).
fn flow_base() -> String {
    std::env::var("FLOW_URL")
        .ok()
        .map(|s| s.trim().trim_end_matches('/').to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "http://127.0.0.1:8787".to_string())
}

/// Poke the engine to re-read graphs after an edit (best-effort, fire-and-forget).
async fn poke_reload() {
    let _ = reqwest::Client::new()
        .post(format!("{}/graphs/reload", flow_base()))
        .send()
        .await;
}

// ---- mutation helper --------------------------------------------------------

/// Load the tenant's graphs, run `f` over the in-memory `Workflows`, persist
/// replace-all, and poke the engine. `f`'s `Err` becomes a 400.
async fn mutate<F, T>(st: &AppState, tid: Uuid, f: F) -> Result<T, ApiErr>
where
    F: FnOnce(&mut Workflows) -> Result<T, String> + Send + 'static,
    T: Send + 'static,
{
    let db = st.db.clone();
    let (out, graphs) = blocking(move || {
        let mut wf = Workflows::from_graphs(workflows::repo::load(&db, tid));
        let out = f(&mut wf)?;
        workflows::repo::save(&db, tid, &wf.graphs);
        Ok::<(T, Vec<WfGraph>), String>((out, wf.graphs))
    })
    .await?
    .map_err(|e| err(StatusCode::BAD_REQUEST, e))?;
    poke_reload().await;
    sync_tracking_config(st, tid, &graphs).await;
    Ok(out)
}

/// Tracker-tuning defaults — must match `sentinel-edge`'s own `default_min_hits`/`default_max_age`/
/// etc. (`sentinel-edge/src/tracking.rs`) and the property panel's displayed defaults
/// (`sentinel-app`'s `effectiveKind`/`chooseEvent`) — all three copies of these numbers need to
/// move together if ever re-tuned.
const DEFAULT_MIN_HITS: u32 = 2;
const DEFAULT_MAX_AGE: u32 = 25;
const DEFAULT_IOU_THRESH: f32 = 0.3;
const DEFAULT_HIGH_THRESH: f32 = 0.5;
const DEFAULT_LOW_THRESH: f32 = 0.1;

/// Read a numeric config value, accepting both a JSON number and a numeric string (the editor
/// stores node config fields as strings) — mirrors `vision.rs::run_recognize_person`'s
/// `max_distance` parsing.
fn config_num(config: &Value, key: &str) -> Option<f64> {
    config.get(key).and_then(|v| v.as_f64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok())))
}

/// One (camera, class) job's accumulated config while walking the graphs.
#[derive(Default)]
struct JobAcc {
    lines: Vec<Value>,
    /// This job's zone (region of interest) + how it's judged, if any `object_entered`/
    /// `object_exited` node for it has one drawn. At most one is meaningful per job — see
    /// `sync_tracking_config`'s doc.
    zone: Option<(Value, String)>,
    /// Tracker tuning + debug-logging flag for this job — set on every visit to this
    /// `(camera, class)` key (see the loop body), so with multiple nodes sharing a job the last
    /// one visited wins, same "last wins" semantics as `zone`.
    min_hits: u32,
    max_age: u32,
    iou_thresh: f32,
    high_thresh: f32,
    low_thresh: f32,
    debug: bool,
}

/// Extract every enabled graph's `OnEvent("object_entered"/"object_exited"/"line_crossed")`
/// node with a resolved `camera` (a config literal, or wired from `GetCameraByName` — see
/// `Graph::resolve_camera_selector`) and `class` configured (card: edge tracking) into this
/// tenant's box(es) `tracking_config.trackers` — the edge's next boot (or restart) then runs
/// exactly the jobs the graphs currently declare. Two nodes naming the same camera+class share
/// one job: their `region` inputs (wired from a `Region` node — see `Graph::resolve_region`) are
/// merged as lines (`line_crossed`, 2-point regions) or the job's zone (entered/exited, 3+-point
/// regions; whichever node's is seen last wins — drawing one on more than one node for the same
/// camera+class is a user-error the editor should eventually warn about, but isn't rejected
/// here). Runs after every graph mutation via `mutate`'s single choke point. Best-effort: logged,
/// never fails the request — a sync hiccup just means the edge keeps its last-known config.
async fn sync_tracking_config(st: &AppState, tid: Uuid, graphs: &[WfGraph]) {
    use std::collections::BTreeMap;

    let mut jobs: BTreeMap<(String, String), JobAcc> = BTreeMap::new();
    for g in graphs.iter().filter(|g| g.enabled) {
        for n in &g.nodes {
            if n.kind != WfNodeKind::OnEvent {
                continue;
            }
            let event = n.config.get("event").and_then(|v| v.as_str()).unwrap_or("");
            if !matches!(event, "object_entered" | "object_exited" | "line_crossed") {
                continue;
            }
            let Some(camera) = g.resolve_camera_selector(n) else { continue }; // not (yet) scoped to a camera
            // The property panel shows "person" as the class dropdown's value before the user
            // ever touches it (sentinel-app's `class` row: `config[key] ?? "person"`) — mirror
            // that display-time fallback here so an untouched-but-scoped OnEvent node still
            // resolves to a real job instead of being silently dropped. An explicitly emptied
            // value ("") still opts the node out.
            let class = n.config.get("class").and_then(|v| v.as_str()).unwrap_or("person").trim();
            if class.is_empty() {
                continue; // explicitly cleared to blank — not scoped to a class
            }
            let acc = jobs.entry((camera.clone(), class.to_string())).or_default();
            // Tuning + debug apply regardless of whether this node has a region wired, so set
            // them before the early-continue below.
            acc.min_hits = config_num(&n.config, "min_hits").map(|v| v as u32).unwrap_or(DEFAULT_MIN_HITS);
            acc.max_age = config_num(&n.config, "max_age").map(|v| v as u32).unwrap_or(DEFAULT_MAX_AGE);
            acc.iou_thresh = config_num(&n.config, "iou_thresh").map(|v| v as f32).unwrap_or(DEFAULT_IOU_THRESH);
            acc.high_thresh = config_num(&n.config, "high_thresh").map(|v| v as f32).unwrap_or(DEFAULT_HIGH_THRESH);
            acc.low_thresh = config_num(&n.config, "low_thresh").map(|v| v as f32).unwrap_or(DEFAULT_LOW_THRESH);
            acc.debug = g.debug;
            let Some((name, points)) = g.resolve_region(n) else { continue }; // no region wired — whole-frame
            if event == "line_crossed" {
                if points.len() == 2 {
                    acc.lines.push(json!({ "camera": camera, "name": name, "p1": points[0], "p2": points[1] }));
                }
            } else if points.len() >= 3 {
                let trigger = n.config.get("zone_trigger").and_then(|v| v.as_str()).unwrap_or("intersects");
                acc.zone = Some((json!({ "camera": camera, "name": name, "points": points }), trigger.to_string()));
            }
        }
    }

    let trackers: Vec<Value> = jobs
        .into_iter()
        .map(|((camera, class), acc)| {
            let mut t = json!({
                "camera": camera,
                "class": class,
                "lines": acc.lines,
                "min_hits": acc.min_hits,
                "max_age": acc.max_age,
                "iou_thresh": acc.iou_thresh,
                "high_thresh": acc.high_thresh,
                "low_thresh": acc.low_thresh,
                "debug": acc.debug,
            });
            if let (Some(obj), Some((zone, trigger))) = (t.as_object_mut(), acc.zone) {
                obj.insert("zone".to_string(), zone);
                obj.insert("zone_trigger".to_string(), json!(trigger));
            }
            t
        })
        .collect();

    let boxes = match store::list_boxes(&st.pool, tid).await {
        Ok(b) => b,
        Err(e) => {
            log::warn!("sync_tracking_config: list_boxes failed: {e}");
            return;
        }
    };
    for b in boxes {
        // Preserve the box's own enabled/detect_ms (set via the tracking-config API, not the
        // graph editor) — only `trackers` is graph-derived and gets fully replaced.
        let current = store::tracking_config_by_id(&st.pool, tid, b.id).await.unwrap_or_default();
        let mut cfg = current.unwrap_or_else(|| json!({}));
        if let Some(obj) = cfg.as_object_mut() {
            obj.entry("enabled").or_insert(json!(false));
            obj.entry("detect_ms").or_insert(json!(400));
            obj.insert("trackers".to_string(), json!(trackers));
        }
        if let Err(e) = store::set_tracking_config(&st.pool, tid, b.id, cfg).await {
            log::warn!("sync_tracking_config: set_tracking_config failed for box {}: {e}", b.id);
        }
    }
}

// ---- read -------------------------------------------------------------------

#[derive(Deserialize)]
struct ListQuery {
    /// Free-text search over name + description + group (case-insensitive).
    q: Option<String>,
    /// Restrict to one group (label).
    group: Option<String>,
}

/// A graph plus its most-recent run summary, for the list. `last_run` is derived (not
/// persisted on the graph) so it never pollutes the saved JSON blob.
#[derive(serde::Serialize)]
struct WfListItem {
    #[serde(flatten)]
    graph: WfGraph,
    last_run: Option<workflows::repo::LastRun>,
}

async fn list(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
    Query(query): Query<ListQuery>,
) -> Result<Json<Vec<WfListItem>>, ApiErr> {
    auth::require(&c, "workflows.read")?;
    let tid = tenant_of(&c)?;
    let db = st.db.clone();
    let items = blocking(move || {
        let graphs = workflows::repo::load(&db, tid);
        let last = workflows::repo::last_run_by_graph(&db, tid);
        let q = query
            .q
            .as_deref()
            .map(|s| s.trim().to_lowercase())
            .filter(|s| !s.is_empty());
        let group = query.group.as_deref().map(str::trim).filter(|s| !s.is_empty());
        graphs
            .into_iter()
            .filter(|g| {
                let hit = q.as_ref().map_or(true, |q| {
                    g.name.to_lowercase().contains(q)
                        || g.description.to_lowercase().contains(q)
                        || g.group.to_lowercase().contains(q)
                });
                let in_group = group.map_or(true, |grp| g.group == grp);
                hit && in_group
            })
            .map(|g| {
                let last_run = last.get(&g.id).cloned();
                WfListItem { graph: g, last_run }
            })
            .collect::<Vec<_>>()
    })
    .await?;
    Ok(Json(items))
}

/// Static node metadata (kinds + typed ports + default config) for the palette.
async fn catalog(UserClaims(c): UserClaims) -> Result<Json<Value>, ApiErr> {
    auth::require(&c, "workflows.read")?;
    let ports = |list: &[workflows::Port]| {
        list.iter()
            .map(|p| json!({ "name": p.name, "type": serde_json::to_value(p.ty).unwrap_or(Value::Null), "required": p.required }))
            .collect::<Vec<_>>()
    };
    let with_exec = |exec: &[workflows::Port], list: &[workflows::Port]| {
        let mut v = ports(exec);
        v.extend(ports(list));
        v
    };
    let exec_in = [workflows::EXEC_IN];
    let kinds = WfNodeKind::ALL
        .iter()
        .map(|k| {
            // Pure data/logic nodes have no exec pins — omit exec_in/exec_out entirely
            // so the UI can hide the exec circles and the engine knows to pull them.
            let (inputs, outputs) = if k.is_pure() {
                (ports(k.input_ports()), ports(k.output_ports()))
            } else {
                (
                    with_exec(&exec_in, k.input_ports()),
                    with_exec(k.exec_out_ports(), k.output_ports()),
                )
            };
            json!({
                "slug": k.slug(),
                "label": k.label(),
                "category": k.category(),
                "inputs": inputs,
                "outputs": outputs,
                "default_config": k.default_config(),
            })
        })
        .collect::<Vec<_>>();
    Ok(Json(json!({ "kinds": kinds })))
}

/// Live run state per graph id (`{ running, current }`) for this tenant from sentinel-flow.
async fn run_states(UserClaims(c): UserClaims) -> Result<Json<Value>, ApiErr> {
    auth::require(&c, "workflows.read")?;
    let tid = tenant_of(&c)?;
    let v = reqwest::Client::new()
        .get(format!("{}/runs?tenant={}", flow_base(), tid))
        .send()
        .await
        .ok()
        .and_then(|r| {
            if r.status().is_success() {
                Some(r)
            } else {
                None
            }
        });
    match v {
        Some(r) => Ok(Json(r.json::<Value>().await.unwrap_or_else(|_| json!({})))),
        None => Ok(Json(json!({}))),
    }
}

#[derive(Deserialize)]
struct HistoryQuery {
    graph: Option<uuid::Uuid>,
    limit: Option<i64>,
}

/// Recent run history for this tenant (newest first), optionally filtered to one graph.
/// Proxies sentinel-flow's persisted run log.
async fn run_history(
    UserClaims(c): UserClaims,
    Query(q): Query<HistoryQuery>,
) -> Result<Json<Value>, ApiErr> {
    auth::require(&c, "workflows.read")?;
    let tid = tenant_of(&c)?;
    let mut url = format!("{}/runs/history?tenant={}", flow_base(), tid);
    if let Some(g) = q.graph {
        url.push_str(&format!("&graph={g}"));
    }
    if let Some(l) = q.limit {
        url.push_str(&format!("&limit={l}"));
    }
    Ok(Json(flow_get_json(&url).await))
}

#[derive(Deserialize)]
struct FrameQuery {
    token: String,
    graph: uuid::Uuid,
    node: u32,
}

/// The latest `ShowImage` debug frame for one of this tenant's `(graph, node)` as
/// `image/jpeg`. The token rides the query param (not a header) so an `<img src>` can
/// authenticate, mirroring [`events_stream`]. Proxies sentinel-flow's out-of-band
/// `/runs/frame`; `404` when no frame is cached for that node.
async fn run_frame(
    State(st): State<AppState>,
    Query(q): Query<FrameQuery>,
) -> Result<axum::response::Response, ApiErr> {
    let claims =
        auth::verify(&st.jwt_secret, &q.token).map_err(|e| err(StatusCode::UNAUTHORIZED, e))?;
    auth::require(&claims, "workflows.read")?;
    let tid = tenant_of(&claims)?;
    let url = format!(
        "{}/runs/frame?tenant={}&graph={}&node={}",
        flow_base(),
        tid,
        q.graph,
        q.node
    );
    let resp = st.http.get(url).send().await.map_err(|e| {
        err(
            StatusCode::BAD_GATEWAY,
            format!("sentinel-flow unreachable: {e}"),
        )
    })?;
    if !resp.status().is_success() {
        return Ok(StatusCode::NOT_FOUND.into_response());
    }
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| err(StatusCode::BAD_GATEWAY, e.to_string()))?;
    Ok(([(axum::http::header::CONTENT_TYPE, "image/jpeg")], bytes).into_response())
}

/// Raw node state JSON for one of this tenant's `(graph, node)` — proxies sentinel-flow.
/// Returns `null` when no state has been written; used by the UI for live TimeGate countdown.
async fn node_state(
    State(st): State<AppState>,
    Query(q): Query<FrameQuery>,
) -> Result<Json<Value>, ApiErr> {
    let claims =
        auth::verify(&st.jwt_secret, &q.token).map_err(|e| err(StatusCode::UNAUTHORIZED, e))?;
    auth::require(&claims, "workflows.read")?;
    let tid = tenant_of(&claims)?;
    let url = format!(
        "{}/runs/node-state?tenant={}&graph={}&node={}",
        flow_base(),
        tid,
        q.graph,
        q.node
    );
    Ok(Json(flow_get_json(&url).await))
}

/// All persisted signal lines for one of this tenant's runs. Proxies sentinel-flow.
async fn run_logs(
    UserClaims(c): UserClaims,
    Path(run_id): Path<Uuid>,
) -> Result<Json<Value>, ApiErr> {
    auth::require(&c, "workflows.read")?;
    let tid = tenant_of(&c)?;
    let url = format!("{}/runs/{}/logs?tenant={}", flow_base(), run_id, tid);
    Ok(Json(flow_get_json(&url).await))
}

#[derive(serde::Deserialize)]
struct ConsoleLogsQuery {
    #[serde(default)]
    limit: Option<i64>,
}

/// Recent node-execution lines across a graph's runs — the editor console seed. Proxies
/// sentinel-flow (live `workflow_run_logs`), NOT the frozen `platform_logs` mirror.
async fn console_logs(
    UserClaims(c): UserClaims,
    Path(id): Path<Uuid>,
    Query(q): Query<ConsoleLogsQuery>,
) -> Result<Json<Value>, ApiErr> {
    auth::require(&c, "workflows.read")?;
    let tid = tenant_of(&c)?;
    let url = format!("{}/graphs/{}/logs?tenant={}&limit={}", flow_base(), id, tid, q.limit.unwrap_or(50));
    Ok(Json(flow_get_json(&url).await))
}

/// GET a sentinel-flow URL and parse its JSON body, defaulting to `[]` on any failure
/// (history/logs are best-effort views).
async fn flow_get_json(url: &str) -> Value {
    match reqwest::Client::new().get(url).send().await {
        Ok(r) if r.status().is_success() => r.json::<Value>().await.unwrap_or_else(|_| json!([])),
        _ => json!([]),
    }
}

// ---- live instances (#95 / #94) --------------------------------------------

/// The live/known instances of one graph (#95 phase 4). Source: distinct `instance_key`s that have
/// saved state or values in `workflow_node_state` (see `repo::list_instances`). `active` is per
/// instance (an armed latent window); `run_id`/`status`/`updated_at` describe the graph's latest run
/// and are the same for every row — runs aren't instance-keyed yet (#95 phase 2), so this is a
/// best-effort annotation, not a per-instance attribution.
async fn instances(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
    Path(id): Path<uuid::Uuid>,
) -> Result<Json<Value>, ApiErr> {
    auth::require(&c, "workflows.read")?;
    let tid = tenant_of(&c)?;
    let db = st.db.clone();
    let out = blocking(move || {
        let instances = workflows::repo::list_instances(&db, tid, id);
        let (run_id, status, updated_at) = match workflows::repo::latest_run_for_graph(&db, tid, id) {
            Some((rid, status, started_at)) => {
                (Some(rid.to_string()), Some(status), Some(started_at))
            }
            None => (None, None, None),
        };
        instances
            .into_iter()
            .map(|i| {
                json!({
                    "instance_key": i.instance_key,
                    "active": i.active,
                    "updated_at": updated_at,
                    "run_id": run_id,
                    "status": status,
                })
            })
            .collect::<Vec<_>>()
    })
    .await?;
    Ok(Json(json!(out)))
}

#[derive(Deserialize)]
struct InstanceKeyQuery {
    /// The instance to read. Absent or empty → the `Single`-scope instance (key `""`).
    key: Option<String>,
}

/// One instance's saved value context: every node's persisted outputs, rendered to short display
/// strings (#94 phase 2). Media never appears as bytes — the codec drops it and `display_value`
/// labels the rest.
async fn instance_values(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
    Path(id): Path<uuid::Uuid>,
    Query(q): Query<InstanceKeyQuery>,
) -> Result<Json<Value>, ApiErr> {
    auth::require(&c, "workflows.read")?;
    let tid = tenant_of(&c)?;
    let key = q.key.unwrap_or_default();
    let db = st.db.clone();
    let out = blocking(move || {
        let graph = workflows::repo::load(&db, tid)
            .into_iter()
            .find(|g| g.id == id);
        let nodes = workflows::repo::instance_node_values(&db, tid, id, &key)
            .into_iter()
            .map(|(nid, vals_json)| {
                let decoded = workflows::values::from_json(&vals_json);
                let mut values = serde_json::Map::new();
                if let Some(pv) = decoded.get(&nid) {
                    for (port, v) in pv {
                        values.insert(
                            port.clone(),
                            Value::String(workflows::values::display_value(v)),
                        );
                    }
                }
                let label = graph.as_ref().and_then(|g| g.node(nid)).map(|n| {
                    n.config
                        .get("label")
                        .and_then(|v| v.as_str())
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| n.kind.label().to_string())
                });
                json!({ "node_id": nid, "label": label, "values": Value::Object(values) })
            })
            .collect::<Vec<_>>();
        json!({ "instance_key": key, "nodes": nodes })
    })
    .await?;
    Ok(Json(out))
}

// ---- event registry + categories (per tenant) ------------------------------

/// The events the editor palette offers: the platform's system events (always available, with
/// payload schema) merged with this tenant's own registered events. System events win on a
/// name clash (they're platform-defined).
async fn list_events(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
) -> Result<Json<Value>, ApiErr> {
    auth::require(&c, "workflows.read")?;
    let tid = tenant_of(&c)?;
    let db = st.db.clone();
    let events = blocking(move || {
        let mut out = workflows::repo::system_events();
        let sys: std::collections::HashSet<String> = out.iter().map(|e| e.name.clone()).collect();
        out.extend(
            workflows::repo::list_events(&db, tid)
                .into_iter()
                .filter(|e| !sys.contains(&e.name)),
        );
        out
    })
    .await?;
    Ok(Json(
        serde_json::to_value(events).unwrap_or_else(|_| json!([])),
    ))
}

#[derive(Deserialize)]
struct CreateEventReq {
    name: String,
    category: Option<String>,
    description: Option<String>,
    /// Typed payload schema: `[{ name, type }]`. Optional (empty = payload-less).
    #[serde(default)]
    fields: Vec<workflows::repo::EventField>,
}

/// Register a new tenant event (idempotent), creating its category if needed.
async fn create_event(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
    Json(b): Json<CreateEventReq>,
) -> Result<StatusCode, ApiErr> {
    auth::require(&c, "workflows.write")?;
    let tid = tenant_of(&c)?;
    let name = b.name.trim().to_string();
    if name.is_empty() {
        return Err(err(StatusCode::BAD_REQUEST, "event name is required"));
    }
    let category = b
        .category
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("General")
        .to_string();
    let description = b
        .description
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let fields = sanitize_event_fields(b.fields)?;
    let db = st.db.clone();
    blocking(move || {
        workflows::repo::create_event(
            &db,
            tid,
            &name,
            &category,
            description.as_deref(),
            None,
            &fields,
        )
    })
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Validate a payload schema: trim names, drop empties, reject the reserved `exec` name,
/// and reject duplicates (each field becomes a port — names must be unique + non-reserved).
fn sanitize_event_fields(
    fields: Vec<workflows::repo::EventField>,
) -> Result<Vec<workflows::repo::EventField>, ApiErr> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::with_capacity(fields.len());
    for mut f in fields {
        f.name = f.name.trim().to_string();
        if f.name.is_empty() {
            continue;
        }
        if f.name.eq_ignore_ascii_case("exec") {
            return Err(err(
                StatusCode::BAD_REQUEST,
                "'exec' is a reserved field name",
            ));
        }
        if !seen.insert(f.name.to_lowercase()) {
            return Err(err(
                StatusCode::BAD_REQUEST,
                format!("duplicate payload field '{}'", f.name),
            ));
        }
        out.push(f);
    }
    Ok(out)
}

/// Remove a registered tenant event.
async fn delete_event(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
    Path(name): Path<String>,
) -> Result<StatusCode, ApiErr> {
    auth::require(&c, "workflows.write")?;
    let tid = tenant_of(&c)?;
    let db = st.db.clone();
    blocking(move || workflows::repo::delete_event(&db, tid, &name)).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// The tenant's event categories (name + source).
async fn list_categories(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
) -> Result<Json<Value>, ApiErr> {
    auth::require(&c, "workflows.read")?;
    let tid = tenant_of(&c)?;
    let db = st.db.clone();
    let cats = blocking(move || workflows::repo::list_categories(&db, tid)).await?;
    Ok(Json(
        serde_json::to_value(cats).unwrap_or_else(|_| json!([])),
    ))
}

/// Create a (user) event category, idempotently.
async fn create_category(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
    Json(b): Json<NameReq>,
) -> Result<StatusCode, ApiErr> {
    auth::require(&c, "workflows.write")?;
    let tid = tenant_of(&c)?;
    let name = b.name.trim().to_string();
    if name.is_empty() {
        return Err(err(StatusCode::BAD_REQUEST, "category name is required"));
    }
    let db = st.db.clone();
    blocking(move || workflows::repo::create_category(&db, tid, &name, "user")).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ---- live event relay (SSE) -------------------------------------------------

#[derive(Deserialize)]
struct EventsQuery {
    token: String,
}

/// Relay run-progress SSE to the browser, **scoped to the caller's tenant**.
/// With Redis: subscribes to `events:<tenant>` pubsub (multi-pod, all flow events).
/// Without Redis: proxies sentinel-flow's internal `/events` SSE and filters by tenant
/// (single-pod fallback — no fan-out, but works without a Redis sidecar).
async fn events_stream(
    State(st): State<AppState>,
    Query(q): Query<EventsQuery>,
) -> Result<Sse<impl futures::Stream<Item = Result<Event, Infallible>>>, ApiErr> {
    let claims =
        auth::verify(&st.jwt_secret, &q.token).map_err(|e| err(StatusCode::UNAUTHORIZED, e))?;
    auth::require(&claims, "workflows.read")?;
    let tid = tenant_of(&claims)?.to_string();

    let (tx, rx) = tokio::sync::mpsc::channel::<Result<Event, Infallible>>(256);

    if let Some(url) = st.redis_url.clone() {
        let chan = format!("events:{tid}");
        tokio::spawn(async move {
            let client = match redis::Client::open(url) {
                Ok(c) => c,
                Err(e) => {
                    log::warn!("events relay: bad redis url: {e}");
                    return;
                }
            };
            let mut pubsub = match client.get_async_pubsub().await {
                Ok(p) => p,
                Err(e) => {
                    log::warn!("events relay: redis pubsub connect failed: {e}");
                    return;
                }
            };
            if let Err(e) = pubsub.subscribe(&chan).await {
                log::warn!("events relay: subscribe {chan} failed: {e}");
                return;
            }
            let mut on_msg = pubsub.on_message();
            loop {
                tokio::select! {
                    // Browser gone (SSE stream dropped → receiver closed). MUST be checked even when
                    // the tenant's channel is idle: otherwise `on_msg.next()` blocks forever and this
                    // Redis pubsub CONNECTION LEAKS — one per page-open/reconnect, which exhausts the
                    // process FD limit (`accept: Too many open files`, os error 24).
                    _ = tx.closed() => return,
                    msg = on_msg.next() => {
                        let Some(msg) = msg else { return }; // pubsub stream ended
                        let Ok(payload) = msg.get_payload::<String>() else { continue };
                        if tx.send(Ok(Event::default().data(payload))).await.is_err() {
                            return; // browser disconnected mid-send
                        }
                    }
                }
            }
        });
    } else {
        // Single-pod fallback: proxy sentinel-flow's internal /events and filter by tenant.
        let flow_events_url = format!("{}/events", flow_base());
        tokio::spawn(async move {
            relay_flow_events(flow_events_url, tid, tx).await;
        });
    }

    let stream = tokio_stream::wrappers::ReceiverStream::new(rx);
    Ok(Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15))))
}

/// HTTP-proxy sentinel-flow's internal SSE `/events` to a browser client, forwarding
/// only events whose `tenant` field matches `tid`. Used as the single-pod fallback when
/// `REDIS_URL` is not configured.
async fn relay_flow_events(
    url: String,
    tenant: String,
    tx: tokio::sync::mpsc::Sender<Result<Event, Infallible>>,
) {
    let resp = match reqwest::Client::new().get(&url).send().await {
        Ok(r) => r,
        Err(e) => {
            log::warn!("events proxy: flow /events unreachable ({url}): {e}");
            return;
        }
    };
    let mut stream = resp.bytes_stream();
    let mut buf = String::new();
    while let Some(item) = stream.next().await {
        let Ok(bytes) = item else { return };
        let Ok(text) = std::str::from_utf8(&bytes) else {
            continue;
        };
        buf.push_str(text);
        // Drain all complete lines from the buffer, forward matching tenant's data events.
        while let Some(nl) = buf.find('\n') {
            let line = buf[..nl].trim_end_matches('\r').to_string();
            buf.drain(..=nl);
            if let Some(data) = line.strip_prefix("data: ") {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(data) {
                    if v.get("tenant").and_then(|t| t.as_str()) == Some(tenant.as_str()) {
                        if tx
                            .send(Ok(Event::default().data(data.to_string())))
                            .await
                            .is_err()
                        {
                            return; // browser disconnected
                        }
                    }
                }
            }
        }
    }
}

// ---- graph lifecycle --------------------------------------------------------

#[derive(Deserialize)]
struct NameReq {
    name: String,
}

async fn create(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
    Json(b): Json<NameReq>,
) -> Result<Json<uuid::Uuid>, ApiErr> {
    auth::require(&c, "workflows.write")?;
    let tid = tenant_of(&c)?;
    let name = if b.name.trim().is_empty() {
        "Untitled".to_string()
    } else {
        b.name.trim().to_string()
    };
    let id = mutate(&st, tid, move |wf| Ok(wf.create_graph(&name))).await?;
    Ok(Json(id))
}

async fn rename(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
    Path(id): Path<uuid::Uuid>,
    Json(b): Json<NameReq>,
) -> Result<StatusCode, ApiErr> {
    auth::require(&c, "workflows.write")?;
    let tid = tenant_of(&c)?;
    mutate(&st, tid, move |wf| {
        wf.graph_mut(id).ok_or("graph not found")?.name = b.name.trim().to_string();
        Ok(())
    })
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct TextReq {
    value: String,
}

async fn set_description(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
    Path(id): Path<uuid::Uuid>,
    Json(b): Json<TextReq>,
) -> Result<StatusCode, ApiErr> {
    auth::require(&c, "workflows.write")?;
    let tid = tenant_of(&c)?;
    mutate(&st, tid, move |wf| {
        wf.graph_mut(id).ok_or("graph not found")?.description = b.value.trim().to_string();
        Ok(())
    })
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn set_group(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
    Path(id): Path<uuid::Uuid>,
    Json(b): Json<TextReq>,
) -> Result<StatusCode, ApiErr> {
    auth::require(&c, "workflows.write")?;
    let tid = tenant_of(&c)?;
    mutate(&st, tid, move |wf| {
        wf.graph_mut(id).ok_or("graph not found")?.group = b.value.trim().to_string();
        Ok(())
    })
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Deep-copy a graph (nodes/edges/trigger/config), name it "<name> (copy)", disabled.
/// Returns the new uuid.
async fn duplicate(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
    Path(id): Path<uuid::Uuid>,
) -> Result<Json<uuid::Uuid>, ApiErr> {
    auth::require(&c, "workflows.write")?;
    let tid = tenant_of(&c)?;
    let new_id = mutate(&st, tid, move |wf| {
        let mut copy = wf.graph(id).ok_or("graph not found")?.clone();
        copy.id = uuid::Uuid::new_v4();
        copy.name = format!("{} (copy)", copy.name);
        copy.enabled = false;
        let new_id = copy.id;
        wf.graphs.push(copy);
        Ok(new_id)
    })
    .await?;
    Ok(Json(new_id))
}

// ---- templates (card #102) --------------------------------------------------

/// A template as the picker sees it — summary only, plus its input variables so the setup form
/// knows what to ask. `global` distinguishes the shared catalog from a tenant's own templates.
#[derive(serde::Serialize)]
struct TemplateSummary {
    id: uuid::Uuid,
    name: String,
    description: String,
    global: bool,
    inputs: Vec<workflows::domain::VariableDecl>,
}

fn to_summary(g: &WfGraph) -> TemplateSummary {
    TemplateSummary {
        id: g.id,
        name: g.name.clone(),
        description: g.description.clone(),
        global: g.global,
        inputs: g.variables.iter().filter(|v| v.param).cloned().collect(),
    }
}

/// The templates a tenant can start from: its **own** (tenant-scoped, `template=true`) plus every
/// **global** template across the platform (a master's shared catalog). De-duped by id.
async fn list_templates(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
) -> Result<Json<Vec<TemplateSummary>>, ApiErr> {
    auth::require(&c, "workflows.read")?;
    let tid = tenant_of(&c)?;
    let db = st.db.clone();
    let out = blocking(move || {
        let mut seen = std::collections::HashSet::new();
        let mut out: Vec<TemplateSummary> = Vec::new();
        // Own templates first, then globals — own wins on an id clash (a tenant can't shadow a
        // global by id in practice, but de-dup keeps the list clean).
        for g in workflows::repo::load(&db, tid).iter().filter(|g| g.template) {
            if seen.insert(g.id) {
                out.push(to_summary(g));
            }
        }
        for (_owner, g) in workflows::repo::load_global_templates(&db) {
            if seen.insert(g.id) {
                out.push(to_summary(&g));
            }
        }
        out
    })
    .await?;
    Ok(Json(out))
}

/// Clone a template — the tenant's own or a global one — into the caller's tenant as a normal,
/// runnable workflow. The copy is not a template and not global, disabled until the user fills its
/// inputs and turns it on.
async fn use_template(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
    Path(id): Path<uuid::Uuid>,
) -> Result<Json<uuid::Uuid>, ApiErr> {
    auth::require(&c, "workflows.write")?;
    let tid = tenant_of(&c)?;
    let db = st.db.clone();
    // Find the source template: the tenant's own, else a global one (which may live in another
    // tenant). Cloning reads it cross-tenant, but only for `global` templates — a tenant can never
    // reach another tenant's private graphs this way.
    let source = blocking(move || {
        if let Some(g) = workflows::repo::load(&db, tid)
            .into_iter()
            .find(|g| g.id == id && g.template)
        {
            return Ok(g);
        }
        workflows::repo::load_global_templates(&db)
            .into_iter()
            .find(|(_t, g)| g.id == id)
            .map(|(_t, g)| g)
            .ok_or_else(|| "template not found".to_string())
    })
    .await?
    .map_err(|e| err(StatusCode::NOT_FOUND, e))?;

    let new_id = mutate(&st, tid, move |wf| {
        let mut copy = source;
        copy.id = uuid::Uuid::new_v4();
        copy.enabled = false;
        copy.template = false;
        copy.global = false;
        let new_id = copy.id;
        wf.graphs.push(copy);
        Ok(new_id)
    })
    .await?;
    Ok(Json(new_id))
}

/// Mark a graph as a template (or unmark it). Tenant-scoped — a user's own template.
async fn set_template(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
    Path(id): Path<uuid::Uuid>,
    Json(b): Json<BoolReq>,
) -> Result<StatusCode, ApiErr> {
    auth::require(&c, "workflows.write")?;
    let tid = tenant_of(&c)?;
    mutate(&st, tid, move |wf| {
        let g = wf.graph_mut(id).ok_or("graph not found")?;
        g.template = b.value;
        // Unmarking a template can't leave it globally published.
        if !b.value {
            g.global = false;
        }
        Ok(())
    })
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Publish a template to the **global** catalog (or unpublish it). **Master only** — a regular user
/// can make their own templates but not shove one onto every tenant.
async fn set_global(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
    Path(id): Path<uuid::Uuid>,
    Json(b): Json<BoolReq>,
) -> Result<StatusCode, ApiErr> {
    if !c.master {
        return Err(err(
            StatusCode::FORBIDDEN,
            "only a platform master can publish a global template",
        ));
    }
    let tid = tenant_of(&c)?;
    mutate(&st, tid, move |wf| {
        let g = wf.graph_mut(id).ok_or("graph not found")?;
        // A global template is still a template.
        if b.value {
            g.template = true;
        }
        g.global = b.value;
        Ok(())
    })
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn remove(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
    Path(id): Path<uuid::Uuid>,
) -> Result<StatusCode, ApiErr> {
    auth::require(&c, "workflows.write")?;
    let tid = tenant_of(&c)?;
    mutate(&st, tid, move |wf| {
        wf.remove_graph(id);
        Ok(())
    })
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct TriggerReq {
    kind: String,
    every_secs: Option<u64>,
    /// Present when `kind == "schedule"` (#111) — the calendar schedule config.
    #[serde(default)]
    schedule: Option<Schedule>,
}

async fn set_trigger(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
    Path(id): Path<uuid::Uuid>,
    Json(b): Json<TriggerReq>,
) -> Result<StatusCode, ApiErr> {
    auth::require(&c, "workflows.write")?;
    let tid = tenant_of(&c)?;
    mutate(&st, tid, move |wf| {
        let trigger = match b.kind.as_str() {
            "manual" => Trigger::Manual,
            "cron" => Trigger::Cron {
                every_secs: b.every_secs.unwrap_or(60).max(1),
            },
            "schedule" => Trigger::Schedule(
                b.schedule.clone().ok_or("schedule config is required for a scheduled trigger")?,
            ),
            other => return Err(format!("unknown trigger kind '{other}'")),
        };
        let g = wf.graph_mut(id).ok_or("graph not found")?;
        g.trigger = trigger;
        // A trigger change always stops the graph — it never starts firing on a config change.
        g.enabled = false;
        Ok(())
    })
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct ConcurrencyReq {
    kind: String,
    max: Option<u32>,
}

async fn set_concurrency(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
    Path(id): Path<uuid::Uuid>,
    Json(b): Json<ConcurrencyReq>,
) -> Result<StatusCode, ApiErr> {
    auth::require(&c, "workflows.write")?;
    let tid = tenant_of(&c)?;
    mutate(&st, tid, move |wf| {
        let policy = match b.kind.as_str() {
            "skip" => ConcurrencyPolicy::SkipIfRunning,
            "queue" => ConcurrencyPolicy::QueueSingle {
                max_queued: b.max.unwrap_or(1).max(1),
            },
            "concurrent" => ConcurrencyPolicy::Concurrent {
                max_concurrent: b.max.unwrap_or(2).max(1),
            },
            other => return Err(format!("unknown concurrency kind '{other}'")),
        };
        wf.graph_mut(id).ok_or("graph not found")?.concurrency = policy;
        Ok(())
    })
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct InstanceScopeReq {
    kind: String,
}

/// Set what a graph runs one of (#95). `single` = one instance per graph (today's default);
/// `per_camera` = one instance per camera, so two cameras record + stop independently. Scopes the
/// concurrency gate, each latent window, and the saved value context — see [`InstanceScope`].
async fn set_instance_scope(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
    Path(id): Path<uuid::Uuid>,
    Json(b): Json<InstanceScopeReq>,
) -> Result<StatusCode, ApiErr> {
    auth::require(&c, "workflows.write")?;
    let tid = tenant_of(&c)?;
    mutate(&st, tid, move |wf| {
        let scope = match b.kind.as_str() {
            "single" => InstanceScope::Single,
            "per_camera" => InstanceScope::PerCamera,
            other => return Err(format!("unknown instance scope '{other}'")),
        };
        wf.graph_mut(id).ok_or("graph not found")?.instance_scope = scope;
        Ok(())
    })
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct ViewportReq {
    x: f64,
    y: f64,
    zoom: f64,
}

/// Replace a graph's variable declarations (the Variables panel). Get/Set Variable nodes that
/// reference a removed/renamed variable are left as-is — the editor re-syncs/warns separately.
async fn set_variables(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
    Path(id): Path<uuid::Uuid>,
    Json(b): Json<Vec<VariableDecl>>,
) -> Result<StatusCode, ApiErr> {
    auth::require(&c, "workflows.write")?;
    let tid = tenant_of(&c)?;
    mutate(&st, tid, move |wf| {
        wf.graph_mut(id).ok_or("graph not found")?.variables = b;
        Ok(())
    })
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn set_viewport(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
    Path(id): Path<uuid::Uuid>,
    Json(b): Json<ViewportReq>,
) -> Result<StatusCode, ApiErr> {
    auth::require(&c, "workflows.write")?;
    let tid = tenant_of(&c)?;
    mutate(&st, tid, move |wf| {
        wf.graph_mut(id).ok_or("graph not found")?.viewport = Viewport {
            x: b.x,
            y: b.y,
            zoom: b.zoom,
        };
        Ok(())
    })
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Persist the visual wire bend handles (card #1) so the user's wire organisation survives a
/// reload. Purely cosmetic — replaces the whole map for the graph.
async fn set_edge_vias(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
    Path(id): Path<uuid::Uuid>,
    Json(b): Json<std::collections::HashMap<String, Vec<[i32; 2]>>>,
) -> Result<StatusCode, ApiErr> {
    auth::require(&c, "workflows.write")?;
    let tid = tenant_of(&c)?;
    mutate(&st, tid, move |wf| {
        wf.graph_mut(id).ok_or("graph not found")?.edge_vias = b;
        Ok(())
    })
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct BoolReq {
    value: bool,
}

async fn set_enabled(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
    Path(id): Path<uuid::Uuid>,
    Json(b): Json<BoolReq>,
) -> Result<StatusCode, ApiErr> {
    auth::require(&c, "workflows.write")?;
    let tid = tenant_of(&c)?;
    mutate(&st, tid, move |wf| {
        wf.graph_mut(id).ok_or("graph not found")?.enabled = b.value;
        Ok(())
    })
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn set_run_orphans(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
    Path(id): Path<uuid::Uuid>,
    Json(b): Json<BoolReq>,
) -> Result<StatusCode, ApiErr> {
    auth::require(&c, "workflows.write")?;
    let tid = tenant_of(&c)?;
    mutate(&st, tid, move |wf| {
        wf.graph_mut(id).ok_or("graph not found")?.run_orphans = b.value;
        Ok(())
    })
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn set_debug(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
    Path(id): Path<uuid::Uuid>,
    Json(b): Json<BoolReq>,
) -> Result<StatusCode, ApiErr> {
    auth::require(&c, "workflows.write")?;
    let tid = tenant_of(&c)?;
    mutate(&st, tid, move |wf| {
        wf.graph_mut(id).ok_or("graph not found")?.debug = b.value;
        Ok(())
    })
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

// ---- nodes + edges ----------------------------------------------------------

#[derive(Deserialize)]
struct AddNodeReq {
    kind: String,
    x: i32,
    y: i32,
}

async fn add_node(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
    Path(id): Path<uuid::Uuid>,
    Json(b): Json<AddNodeReq>,
) -> Result<Json<u32>, ApiErr> {
    auth::require(&c, "workflows.write")?;
    let tid = tenant_of(&c)?;
    let nid = mutate(&st, tid, move |wf| {
        let kind = WfNodeKind::from_slug(&b.kind)
            .ok_or_else(|| format!("unknown node kind '{}'", b.kind))?;
        Ok(wf
            .graph_mut(id)
            .ok_or("graph not found")?
            .add_node(kind, b.x, b.y))
    })
    .await?;
    Ok(Json(nid))
}

async fn remove_node(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
    Path((id, nid)): Path<(uuid::Uuid, u32)>,
) -> Result<StatusCode, ApiErr> {
    auth::require(&c, "workflows.write")?;
    let tid = tenant_of(&c)?;
    mutate(&st, tid, move |wf| {
        wf.graph_mut(id).ok_or("graph not found")?.remove_node(nid);
        Ok(())
    })
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct MoveReq {
    x: i32,
    y: i32,
}

async fn move_node(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
    Path((id, nid)): Path<(uuid::Uuid, u32)>,
    Json(b): Json<MoveReq>,
) -> Result<StatusCode, ApiErr> {
    auth::require(&c, "workflows.write")?;
    let tid = tenant_of(&c)?;
    mutate(&st, tid, move |wf| {
        let g = wf.graph_mut(id).ok_or("graph not found")?;
        let n = g.node_mut(nid).ok_or("node not found")?;
        n.x = b.x;
        n.y = b.y;
        Ok(())
    })
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn set_node_config(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
    Path((id, nid)): Path<(uuid::Uuid, u32)>,
    Json(config): Json<Value>,
) -> Result<StatusCode, ApiErr> {
    auth::require(&c, "workflows.write")?;
    let tid = tenant_of(&c)?;
    mutate(&st, tid, move |wf| {
        let g = wf.graph_mut(id).ok_or("graph not found")?;
        g.node_mut(nid).ok_or("node not found")?.config = config;
        Ok(())
    })
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// The editable canvas surface for the explicit-Save flow (#122): the editor holds all node/edge/
/// position/config edits LOCALLY and writes the whole graph in one shot on Save. Replaces the
/// canvas fields; graph METADATA (name, trigger, enabled, concurrency, variables, …) is preserved —
/// those have their own controls.
#[derive(serde::Deserialize)]
struct GraphSave {
    nodes: Vec<app_core::workflows::domain::GraphNode>,
    edges: Vec<app_core::workflows::domain::Edge>,
    #[serde(default)]
    viewport: Option<app_core::workflows::domain::Viewport>,
    #[serde(default)]
    edge_vias: Option<std::collections::HashMap<String, Vec<[i32; 2]>>>,
    #[serde(default)]
    next_id: Option<u32>,
}

async fn save_graph(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
    Path(id): Path<uuid::Uuid>,
    Json(body): Json<GraphSave>,
) -> Result<StatusCode, ApiErr> {
    auth::require(&c, "workflows.write")?;
    let tid = tenant_of(&c)?;
    mutate(&st, tid, move |wf| {
        let g = wf.graph_mut(id).ok_or("graph not found")?;
        g.nodes = body.nodes;
        g.edges = body.edges;
        if let Some(vp) = body.viewport {
            g.viewport = vp;
        }
        if let Some(vias) = body.edge_vias {
            g.edge_vias = vias;
        }
        if let Some(n) = body.next_id {
            g.next_id = n;
        }
        Ok(())
    })
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct MemoizeReq {
    memoize: bool,
}

async fn set_node_memoize(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
    Path((id, nid)): Path<(uuid::Uuid, u32)>,
    Json(b): Json<MemoizeReq>,
) -> Result<StatusCode, ApiErr> {
    auth::require(&c, "workflows.write")?;
    let tid = tenant_of(&c)?;
    mutate(&st, tid, move |wf| {
        wf.graph_mut(id)
            .ok_or("graph not found")?
            .node_mut(nid)
            .ok_or("node not found")?
            .memoize = b.memoize;
        Ok(())
    })
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct AddEdgeReq {
    from: u32,
    from_port: String,
    to: u32,
    to_port: String,
}

async fn add_edge(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
    Path(id): Path<uuid::Uuid>,
    Json(b): Json<AddEdgeReq>,
) -> Result<StatusCode, ApiErr> {
    auth::require(&c, "workflows.write")?;
    let tid = tenant_of(&c)?;
    mutate(&st, tid, move |wf| {
        wf.graph_mut(id)
            .ok_or("graph not found")?
            .add_edge(b.from, &b.from_port, b.to, &b.to_port)
    })
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn remove_edge(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
    Path((id, idx)): Path<(uuid::Uuid, usize)>,
) -> Result<StatusCode, ApiErr> {
    auth::require(&c, "workflows.write")?;
    let tid = tenant_of(&c)?;
    mutate(&st, tid, move |wf| {
        wf.graph_mut(id).ok_or("graph not found")?.remove_edge(idx);
        Ok(())
    })
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

// ---- run --------------------------------------------------------------------

/// Run-now: ask sentinel-flow to fire this tenant's graph immediately. The flow pod is
/// multi-tenant, so the caller's tenant id is passed through and the run is bound to that
/// tenant's edge.
async fn run(
    UserClaims(c): UserClaims,
    Path(id): Path<uuid::Uuid>,
    // Optional per-launch input values (#102), `{ "inputs": { "<var>": "<value>" } }`. Absent for a
    // plain run. `Option<Json<_>>` so a body-less POST still works.
    body: Option<Json<serde_json::Value>>,
) -> Result<Json<String>, ApiErr> {
    auth::require(&c, "workflows.run")?;
    let tid = tenant_of(&c)?;
    let inputs = body
        .and_then(|Json(v)| v.get("inputs").cloned())
        .unwrap_or_else(|| json!({}));
    let resp = reqwest::Client::new()
        .post(format!("{}/runs", flow_base()))
        .json(&json!({ "graph_id": id, "tenant_id": tid, "inputs": inputs }))
        .send()
        .await;
    match resp {
        Ok(r) if r.status().is_success() => Ok(Json("Run accepted.".into())),
        Ok(r) if r.status() == reqwest::StatusCode::NOT_FOUND => {
            Err(err(StatusCode::NOT_FOUND, "graph not found"))
        }
        Ok(r) => Err(err(
            StatusCode::BAD_GATEWAY,
            format!("sentinel-flow error: {}", r.status()),
        )),
        Err(e) => Err(err(
            StatusCode::BAD_GATEWAY,
            format!("sentinel-flow unreachable: {e}"),
        )),
    }
}
