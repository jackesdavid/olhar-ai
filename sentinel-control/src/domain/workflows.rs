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
    self, ConcurrencyPolicy, Graph as WfGraph, NodeKind as WfNodeKind, Trigger, Viewport, Workflows,
};

use super::{blocking, err, tenant_of, ApiErr};
use crate::auth::{self, UserClaims};
use crate::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/workflows", get(list).post(create))
        .route("/workflows/catalog", get(catalog))
        .route("/workflows/runs", get(run_states))
        .route("/workflows/runs/history", get(run_history))
        .route("/workflows/runs/frame", get(run_frame))
        .route("/workflows/runs/:run_id/logs", get(run_logs))
        .route("/workflows/events", get(events_stream))
        .route("/workflows/event-registry", get(list_events).post(create_event))
        .route("/workflows/event-registry/:name", axum::routing::delete(delete_event))
        .route("/workflows/event-categories", get(list_categories).post(create_category))
        .route("/workflows/:id", axum::routing::patch(rename).delete(remove))
        .route("/workflows/:id/trigger", put(set_trigger))
        .route("/workflows/:id/concurrency", put(set_concurrency))
        .route("/workflows/:id/viewport", put(set_viewport))
        .route("/workflows/:id/enabled", put(set_enabled))
        .route("/workflows/:id/run-orphans", put(set_run_orphans))
        .route("/workflows/:id/debug", put(set_debug))
        .route("/workflows/:id/run", post(run))
        .route("/workflows/:id/nodes", post(add_node))
        .route("/workflows/:id/nodes/:nid", axum::routing::delete(remove_node))
        .route("/workflows/:id/nodes/:nid/move", put(move_node))
        .route("/workflows/:id/nodes/:nid/config", put(set_node_config))
        .route("/workflows/:id/nodes/:nid/memoize", put(set_node_memoize))
        .route("/workflows/:id/edges", post(add_edge))
        .route("/workflows/:id/edges/:idx", axum::routing::delete(remove_edge))
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
    let _ = reqwest::Client::new().post(format!("{}/graphs/reload", flow_base())).send().await;
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
    let out = blocking(move || {
        let mut wf = Workflows::from_graphs(workflows::repo::load(&db, tid));
        let out = f(&mut wf)?;
        workflows::repo::save(&db, tid, &wf.graphs);
        Ok::<T, String>(out)
    })
    .await?
    .map_err(|e| err(StatusCode::BAD_REQUEST, e))?;
    poke_reload().await;
    Ok(out)
}

// ---- read -------------------------------------------------------------------

async fn list(UserClaims(c): UserClaims, State(st): State<AppState>) -> Result<Json<Vec<WfGraph>>, ApiErr> {
    auth::require(&c, "workflows.read")?;
    let tid = tenant_of(&c)?;
    let db = st.db.clone();
    Ok(Json(blocking(move || workflows::repo::load(&db, tid)).await?))
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
            json!({
                "slug": k.slug(),
                "label": k.label(),
                "category": k.category(),
                "inputs": with_exec(&exec_in, k.input_ports()),
                "outputs": with_exec(k.exec_out_ports(), k.output_ports()),
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
        .and_then(|r| if r.status().is_success() { Some(r) } else { None });
    match v {
        Some(r) => Ok(Json(r.json::<Value>().await.unwrap_or_else(|_| json!({})))),
        None => Ok(Json(json!({}))),
    }
}

#[derive(Deserialize)]
struct HistoryQuery {
    graph: Option<u32>,
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
    graph: u32,
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
    let claims = auth::verify(&st.jwt_secret, &q.token).map_err(|e| err(StatusCode::UNAUTHORIZED, e))?;
    auth::require(&claims, "workflows.read")?;
    let tid = tenant_of(&claims)?;
    let url = format!("{}/runs/frame?tenant={}&graph={}&node={}", flow_base(), tid, q.graph, q.node);
    let resp = st
        .http
        .get(url)
        .send()
        .await
        .map_err(|e| err(StatusCode::BAD_GATEWAY, format!("sentinel-flow unreachable: {e}")))?;
    if !resp.status().is_success() {
        return Ok(StatusCode::NOT_FOUND.into_response());
    }
    let bytes = resp.bytes().await.map_err(|e| err(StatusCode::BAD_GATEWAY, e.to_string()))?;
    Ok(([(axum::http::header::CONTENT_TYPE, "image/jpeg")], bytes).into_response())
}

/// All persisted signal lines for one of this tenant's runs. Proxies sentinel-flow.
async fn run_logs(UserClaims(c): UserClaims, Path(run_id): Path<Uuid>) -> Result<Json<Value>, ApiErr> {
    auth::require(&c, "workflows.read")?;
    let tid = tenant_of(&c)?;
    let url = format!("{}/runs/{}/logs?tenant={}", flow_base(), run_id, tid);
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

// ---- event registry + categories (per tenant) ------------------------------

/// The tenant's registered events (name + category + source), for the editor palette.
async fn list_events(UserClaims(c): UserClaims, State(st): State<AppState>) -> Result<Json<Value>, ApiErr> {
    auth::require(&c, "workflows.read")?;
    let tid = tenant_of(&c)?;
    let db = st.db.clone();
    let events = blocking(move || workflows::repo::list_events(&db, tid)).await?;
    Ok(Json(serde_json::to_value(events).unwrap_or_else(|_| json!([]))))
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
    let category = b.category.as_deref().map(str::trim).filter(|s| !s.is_empty()).unwrap_or("General").to_string();
    let description = b.description.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    let fields = sanitize_event_fields(b.fields)?;
    let db = st.db.clone();
    blocking(move || workflows::repo::create_event(&db, tid, &name, &category, description.as_deref(), None, &fields)).await?;
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
            return Err(err(StatusCode::BAD_REQUEST, "'exec' is a reserved field name"));
        }
        if !seen.insert(f.name.to_lowercase()) {
            return Err(err(StatusCode::BAD_REQUEST, format!("duplicate payload field '{}'", f.name)));
        }
        out.push(f);
    }
    Ok(out)
}

/// Remove a registered tenant event.
async fn delete_event(UserClaims(c): UserClaims, State(st): State<AppState>, Path(name): Path<String>) -> Result<StatusCode, ApiErr> {
    auth::require(&c, "workflows.write")?;
    let tid = tenant_of(&c)?;
    let db = st.db.clone();
    blocking(move || workflows::repo::delete_event(&db, tid, &name)).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// The tenant's event categories (name + source).
async fn list_categories(UserClaims(c): UserClaims, State(st): State<AppState>) -> Result<Json<Value>, ApiErr> {
    auth::require(&c, "workflows.read")?;
    let tid = tenant_of(&c)?;
    let db = st.db.clone();
    let cats = blocking(move || workflows::repo::list_categories(&db, tid)).await?;
    Ok(Json(serde_json::to_value(cats).unwrap_or_else(|_| json!([]))))
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

/// Relay run-progress SSE to the browser, **scoped to the caller's tenant**, off the Redis
/// event fabric. The editor opens an `EventSource` here (which can't send headers, so the JWT
/// rides the `token` query param). We subscribe to `events:<tenant>`, which carries EVERY flow
/// pod's events (multi-pod). The channel name isolates the tenant, so no per-message filter is
/// needed. Lossy by design (final state is re-fetchable via REST).
async fn events_stream(
    State(st): State<AppState>,
    Query(q): Query<EventsQuery>,
) -> Result<Sse<impl futures::Stream<Item = Result<Event, Infallible>>>, ApiErr> {
    let claims = auth::verify(&st.jwt_secret, &q.token).map_err(|e| err(StatusCode::UNAUTHORIZED, e))?;
    auth::require(&claims, "workflows.read")?;
    let tid = tenant_of(&claims)?.to_string();

    let Some(url) = st.redis_url.clone() else {
        return Err(err(StatusCode::BAD_GATEWAY, "event fabric unavailable (REDIS_URL not set)"));
    };
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<Event, Infallible>>(256);
    let chan = format!("events:{tid}");
    tokio::spawn(async move {
        let client = match redis::Client::open(url) {
            Ok(c) => c,
            Err(e) => { log::warn!("events relay: bad redis url: {e}"); return; }
        };
        let mut pubsub = match client.get_async_pubsub().await {
            Ok(p) => p,
            Err(e) => { log::warn!("events relay: redis pubsub connect failed: {e}"); return; }
        };
        if let Err(e) = pubsub.subscribe(&chan).await {
            log::warn!("events relay: subscribe {chan} failed: {e}");
            return;
        }
        let mut on_msg = pubsub.on_message();
        while let Some(msg) = on_msg.next().await {
            let Ok(payload) = msg.get_payload::<String>() else { continue };
            if tx.send(Ok(Event::default().data(payload))).await.is_err() {
                return; // browser disconnected
            }
        }
    });

    let stream = tokio_stream::wrappers::ReceiverStream::new(rx);
    Ok(Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15))))
}

// ---- graph lifecycle --------------------------------------------------------

#[derive(Deserialize)]
struct NameReq {
    name: String,
}

async fn create(UserClaims(c): UserClaims, State(st): State<AppState>, Json(b): Json<NameReq>) -> Result<Json<u32>, ApiErr> {
    auth::require(&c, "workflows.write")?;
    let tid = tenant_of(&c)?;
    let name = if b.name.trim().is_empty() { "Untitled".to_string() } else { b.name.trim().to_string() };
    let id = mutate(&st, tid, move |wf| Ok(wf.create_graph(&name))).await?;
    Ok(Json(id))
}

async fn rename(UserClaims(c): UserClaims, State(st): State<AppState>, Path(id): Path<u32>, Json(b): Json<NameReq>) -> Result<StatusCode, ApiErr> {
    auth::require(&c, "workflows.write")?;
    let tid = tenant_of(&c)?;
    mutate(&st, tid, move |wf| {
        wf.graph_mut(id).ok_or("graph not found")?.name = b.name.trim().to_string();
        Ok(())
    })
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn remove(UserClaims(c): UserClaims, State(st): State<AppState>, Path(id): Path<u32>) -> Result<StatusCode, ApiErr> {
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
}

async fn set_trigger(UserClaims(c): UserClaims, State(st): State<AppState>, Path(id): Path<u32>, Json(b): Json<TriggerReq>) -> Result<StatusCode, ApiErr> {
    auth::require(&c, "workflows.write")?;
    let tid = tenant_of(&c)?;
    mutate(&st, tid, move |wf| {
        let trigger = match b.kind.as_str() {
            "manual" => Trigger::Manual,
            "cron" => Trigger::Cron { every_secs: b.every_secs.unwrap_or(60).max(1) },
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

async fn set_concurrency(UserClaims(c): UserClaims, State(st): State<AppState>, Path(id): Path<u32>, Json(b): Json<ConcurrencyReq>) -> Result<StatusCode, ApiErr> {
    auth::require(&c, "workflows.write")?;
    let tid = tenant_of(&c)?;
    mutate(&st, tid, move |wf| {
        let policy = match b.kind.as_str() {
            "skip" => ConcurrencyPolicy::SkipIfRunning,
            "queue" => ConcurrencyPolicy::QueueSingle { max_queued: b.max.unwrap_or(1).max(1) },
            "concurrent" => ConcurrencyPolicy::Concurrent { max_concurrent: b.max.unwrap_or(2).max(1) },
            other => return Err(format!("unknown concurrency kind '{other}'")),
        };
        wf.graph_mut(id).ok_or("graph not found")?.concurrency = policy;
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

async fn set_viewport(UserClaims(c): UserClaims, State(st): State<AppState>, Path(id): Path<u32>, Json(b): Json<ViewportReq>) -> Result<StatusCode, ApiErr> {
    auth::require(&c, "workflows.write")?;
    let tid = tenant_of(&c)?;
    mutate(&st, tid, move |wf| {
        wf.graph_mut(id).ok_or("graph not found")?.viewport = Viewport { x: b.x, y: b.y, zoom: b.zoom };
        Ok(())
    })
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct BoolReq {
    value: bool,
}

async fn set_enabled(UserClaims(c): UserClaims, State(st): State<AppState>, Path(id): Path<u32>, Json(b): Json<BoolReq>) -> Result<StatusCode, ApiErr> {
    auth::require(&c, "workflows.write")?;
    let tid = tenant_of(&c)?;
    mutate(&st, tid, move |wf| {
        wf.graph_mut(id).ok_or("graph not found")?.enabled = b.value;
        Ok(())
    })
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn set_run_orphans(UserClaims(c): UserClaims, State(st): State<AppState>, Path(id): Path<u32>, Json(b): Json<BoolReq>) -> Result<StatusCode, ApiErr> {
    auth::require(&c, "workflows.write")?;
    let tid = tenant_of(&c)?;
    mutate(&st, tid, move |wf| {
        wf.graph_mut(id).ok_or("graph not found")?.run_orphans = b.value;
        Ok(())
    })
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn set_debug(UserClaims(c): UserClaims, State(st): State<AppState>, Path(id): Path<u32>, Json(b): Json<BoolReq>) -> Result<StatusCode, ApiErr> {
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

async fn add_node(UserClaims(c): UserClaims, State(st): State<AppState>, Path(id): Path<u32>, Json(b): Json<AddNodeReq>) -> Result<Json<u32>, ApiErr> {
    auth::require(&c, "workflows.write")?;
    let tid = tenant_of(&c)?;
    let nid = mutate(&st, tid, move |wf| {
        let kind = WfNodeKind::from_slug(&b.kind).ok_or_else(|| format!("unknown node kind '{}'", b.kind))?;
        Ok(wf.graph_mut(id).ok_or("graph not found")?.add_node(kind, b.x, b.y))
    })
    .await?;
    Ok(Json(nid))
}

async fn remove_node(UserClaims(c): UserClaims, State(st): State<AppState>, Path((id, nid)): Path<(u32, u32)>) -> Result<StatusCode, ApiErr> {
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

async fn move_node(UserClaims(c): UserClaims, State(st): State<AppState>, Path((id, nid)): Path<(u32, u32)>, Json(b): Json<MoveReq>) -> Result<StatusCode, ApiErr> {
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

async fn set_node_config(UserClaims(c): UserClaims, State(st): State<AppState>, Path((id, nid)): Path<(u32, u32)>, Json(config): Json<Value>) -> Result<StatusCode, ApiErr> {
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

#[derive(Deserialize)]
struct MemoizeReq {
    memoize: bool,
}

async fn set_node_memoize(UserClaims(c): UserClaims, State(st): State<AppState>, Path((id, nid)): Path<(u32, u32)>, Json(b): Json<MemoizeReq>) -> Result<StatusCode, ApiErr> {
    auth::require(&c, "workflows.write")?;
    let tid = tenant_of(&c)?;
    mutate(&st, tid, move |wf| {
        wf.graph_mut(id).ok_or("graph not found")?.node_mut(nid).ok_or("node not found")?.memoize = b.memoize;
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

async fn add_edge(UserClaims(c): UserClaims, State(st): State<AppState>, Path(id): Path<u32>, Json(b): Json<AddEdgeReq>) -> Result<StatusCode, ApiErr> {
    auth::require(&c, "workflows.write")?;
    let tid = tenant_of(&c)?;
    mutate(&st, tid, move |wf| {
        wf.graph_mut(id).ok_or("graph not found")?.add_edge(b.from, &b.from_port, b.to, &b.to_port)
    })
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn remove_edge(UserClaims(c): UserClaims, State(st): State<AppState>, Path((id, idx)): Path<(u32, usize)>) -> Result<StatusCode, ApiErr> {
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
async fn run(UserClaims(c): UserClaims, Path(id): Path<u32>) -> Result<Json<String>, ApiErr> {
    auth::require(&c, "workflows.run")?;
    let tid = tenant_of(&c)?;
    let resp = reqwest::Client::new()
        .post(format!("{}/runs", flow_base()))
        .json(&json!({ "graph_id": id, "tenant_id": tid }))
        .send()
        .await;
    match resp {
        Ok(r) if r.status().is_success() => Ok(Json("Run accepted.".into())),
        Ok(r) if r.status() == reqwest::StatusCode::NOT_FOUND => Err(err(StatusCode::NOT_FOUND, "graph not found")),
        Ok(r) => Err(err(StatusCode::BAD_GATEWAY, format!("sentinel-flow error: {}", r.status()))),
        Err(e) => Err(err(StatusCode::BAD_GATEWAY, format!("sentinel-flow unreachable: {e}"))),
    }
}