//! The REST + SSE control plane. The UI becomes a client: it lists graphs, polls
//! run state, asks for a manual run (`POST /runs` → 202, async-job style), and
//! subscribes to the live event stream. The engine remains authoritative; SSE is an
//! optimization layered over re-fetchable REST state.
use base64::Engine as _;
use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Json};
use axum::routing::{get, post};
use axum::Router;
use serde::Deserialize;
use serde_json::json;
use tower_http::cors::CorsLayer;
use uuid::Uuid;

use crate::engine;
use crate::state::AppState;

/// Build the router with permissive CORS (the browser UI calls cross-origin).
pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/graphs", get(graphs))
        .route("/graphs/reload", post(reload_graphs))
        .route("/runs", get(run_states).post(create_run))
        .route("/runs/history", get(run_history))
        .route("/runs/frame", get(run_frame))
        .route("/runs/node-state", get(node_state))
        .route("/runs/:run_id/logs", get(run_logs))
        .route("/graphs/:graph_id/logs", get(graph_logs))
        .route("/events", get(events_sse))
        .layer(CorsLayer::permissive())
        .with_state(state)
}

/// Liveness — and a count of loaded graphs so a probe can confirm hydration.
async fn health(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let graphs = state.store.lock().unwrap().len();
    Json(json!({ "status": "ok", "service": "sentinel-flow", "graphs": graphs }))
}

/// All named graphs across all tenants (`{ tenant, graph }`) — the working set.
async fn graphs(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let graphs: Vec<_> = state
        .store
        .lock()
        .unwrap()
        .iter()
        .map(|(t, g)| json!({ "tenant": t, "graph": g }))
        .collect();
    Json(graphs)
}

/// Reload the graph set from Postgres (the UI edits land there out-of-band).
async fn reload_graphs(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    state.reload_graphs();
    let (n, enabled) = {
        let s = state.store.lock().unwrap();
        (s.len(), s.iter().filter(|(_, g)| g.enabled).count())
    };
    // Log the pod + enabled count so a store divergence across replicas (the cause of "some events
    // ran, some didn't") is visible in Datadog: compare `enabled` across pods for the same reload.
    log::info!(
        "graphs reloaded on request — {n} graph(s), {enabled} enabled (pod {})",
        state.pod_name
    );
    (
        StatusCode::OK,
        Json(json!({ "reloaded": n, "enabled": enabled, "pod": state.pod_name })),
    )
}

#[derive(Deserialize)]
struct TenantQuery {
    tenant: Uuid,
}

#[derive(Deserialize)]
struct HistoryQuery {
    tenant: Uuid,
    #[serde(default)]
    graph: Option<uuid::Uuid>,
    #[serde(default)]
    limit: Option<i64>,
}

/// Live run state for one tenant's graphs, keyed by graph id (`{ running, current }`).
/// The control plane passes `?tenant=<uuid>` (from the caller's JWT) so a tenant only
/// sees its own runs.
async fn run_states(
    State(state): State<Arc<AppState>>,
    Query(q): Query<TenantQuery>,
) -> impl IntoResponse {
    let map: std::collections::HashMap<uuid::Uuid, crate::state::GraphActivity> = state
        .activity
        .lock()
        .unwrap()
        .iter()
        .filter(|((t, _), _)| *t == q.tenant)
        .map(|((_, id), a)| (*id, a.clone()))
        .collect();
    Json(map)
}

#[derive(Deserialize)]
struct CreateRun {
    tenant_id: Uuid,
    graph_id: uuid::Uuid,
    /// Per-launch values for the graph's input variables (#102), by variable name. Raw strings —
    /// parsed here against each variable's declared type, exactly as a stored default would be.
    #[serde(default)]
    inputs: std::collections::HashMap<String, String>,
}

/// Request a manual run of a tenant's graph. Async-job style: the engine applies the
/// graph's concurrency policy and runs on a background thread; we return `202 Accepted`
/// immediately and progress streams over `/events`.
async fn create_run(
    State(state): State<Arc<AppState>>,
    Json(body): Json<CreateRun>,
) -> impl IntoResponse {
    // Manual fire honors the graph's concurrency policy (spawn/queue/skip).
    let graph = crate::state::find_graph(&state.store, body.tenant_id, body.graph_id);
    let Some(graph) = graph else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "no such graph" })),
        );
    };
    let policy = graph.concurrency.clone();
    // Raw input strings flow straight through — execute_run parses them against the graph's
    // variable types with the run's real context.
    engine::fire_graph(
        &state,
        body.tenant_id,
        body.graph_id,
        &policy,
        app_core::workflows::TriggerSource::Manual,
        body.inputs,
    );
    (
        StatusCode::ACCEPTED,
        Json(json!({ "graph_id": body.graph_id, "accepted": true })),
    )
}

/// Recent run history for a tenant (newest first), optionally filtered to one graph
/// (`?tenant=<uuid>&graph=<id>&limit=<n>`). Persisted, so it survives restarts.
async fn run_history(
    State(state): State<Arc<AppState>>,
    Query(q): Query<HistoryQuery>,
) -> impl IntoResponse {
    let limit = q.limit.unwrap_or(50).clamp(1, 500);
    let runs = app_core::workflows::repo::list_runs(&state.db, q.tenant, q.graph, limit);
    Json(runs)
}

#[derive(Deserialize)]
struct FrameQuery {
    tenant: Uuid,
    graph: uuid::Uuid,
    node: u32,
}

/// The latest `ShowImage` debug frame for a tenant's `(graph, node)` as raw `image/jpeg`
/// (`?tenant=<uuid>&graph=<id>&node=<id>`), or `404` if none cached. Served out of band so
/// the bytes never head-of-line-block the SSE stream (see [`crate::events`]).
async fn run_frame(
    State(state): State<Arc<AppState>>,
    Query(q): Query<FrameQuery>,
) -> impl IntoResponse {
    // Fast path: in-memory cache (current or very recent run).
    let bytes = state
        .node_frames
        .lock()
        .unwrap()
        .get(&(q.tenant, q.graph, q.node))
        .cloned();
    if let Some(jpeg) = bytes {
        return (
            StatusCode::OK,
            [(axum::http::header::CONTENT_TYPE, "image/jpeg")],
            jpeg,
        )
            .into_response();
    }
    // Fallback: persisted frame from workflow_node_state (survives flow restarts and browser refreshes).
    let db = state.db.clone();
    let saved = tokio::task::spawn_blocking(move || {
        app_core::workflows::repo::get_node_state(&db, q.tenant, q.graph, q.node, "")
    })
    .await
    .ok()
    .flatten();
    match saved
        .and_then(|v| {
            v.get("jpeg_b64")
                .and_then(|b| b.as_str())
                .map(str::to_string)
        })
        .and_then(|b64| base64::engine::general_purpose::STANDARD.decode(b64).ok())
    {
        Some(jpeg) => (
            StatusCode::OK,
            [(axum::http::header::CONTENT_TYPE, "image/jpeg")],
            jpeg,
        )
            .into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

/// Raw `workflow_node_state` JSON for one `(tenant, graph, node)` — used by the UI to
/// display live engine-driven node state (TimeGate countdown, Cooldown last-fired, etc.).
/// Returns `null` when no state has been written yet.
async fn node_state(
    State(state): State<Arc<AppState>>,
    Query(q): Query<FrameQuery>,
) -> impl IntoResponse {
    let db = state.db.clone();
    let st = tokio::task::spawn_blocking(move || {
        app_core::workflows::repo::get_node_state(&db, q.tenant, q.graph, q.node, "")
    })
    .await
    .ok()
    .flatten();
    Json(st.unwrap_or(serde_json::Value::Null))
}

/// All persisted signal lines for one run of a tenant (`?tenant=<uuid>`).
async fn run_logs(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(run_id): axum::extract::Path<Uuid>,
    Query(q): Query<TenantQuery>,
) -> impl IntoResponse {
    let logs = app_core::workflows::repo::list_run_logs(&state.db, q.tenant, run_id);
    Json(logs)
}

/// Recent execution lines across a graph's runs (`?tenant=<uuid>&limit=N`) — the editor console
/// seed. Sourced from the live `workflow_run_logs`, newest first. Reuses `HistoryQuery` for
/// tenant+limit (its `graph` field is unused here — the graph id comes from the path).
async fn graph_logs(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(graph_id): axum::extract::Path<Uuid>,
    Query(q): Query<HistoryQuery>,
) -> impl IntoResponse {
    let logs = app_core::workflows::repo::list_graph_run_logs(
        &state.db,
        q.tenant,
        graph_id,
        q.limit.unwrap_or(50),
    );
    Json(logs)
}

/// Internal SSE feed — all run-progress events from this pod's in-process bus, unfiltered.
/// Consumed by the sentinel-control relay when Redis is not configured (single-pod mode).
/// Not exposed externally (sentinel-flow is not public-facing).
async fn events_sse(
    State(state): State<Arc<AppState>>,
) -> Sse<impl futures::Stream<Item = Result<Event, Infallible>>> {
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<Event, Infallible>>(256);
    let mut bus_rx = state.events.subscribe();
    tokio::spawn(async move {
        loop {
            match bus_rx.recv().await {
                Ok(line) => {
                    if tx.send(Ok(Event::default().data(line))).await.is_err() {
                        return;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
            }
        }
    });
    Sse::new(tokio_stream::wrappers::ReceiverStream::new(rx))
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
}
