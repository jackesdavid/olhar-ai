//! The REST + SSE control plane. The UI becomes a client: it lists graphs, polls
//! run state, asks for a manual run (`POST /runs` → 202, async-job style), and
//! subscribes to the live event stream. The engine remains authoritative; SSE is an
//! optimization layered over re-fetchable REST state.
use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Json};
use axum::routing::{get, post};
use axum::Router;
use futures::Stream;
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;
use tower_http::cors::CorsLayer;

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
        .route("/runs/:run_id/logs", get(run_logs))
        .route("/events", get(events))
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
    let n = state.store.lock().unwrap().len();
    log::info!("graphs reloaded on request — {n} graph(s)");
    (StatusCode::OK, Json(json!({ "reloaded": n })))
}

#[derive(Deserialize)]
struct TenantQuery {
    tenant: Uuid,
}

#[derive(Deserialize)]
struct HistoryQuery {
    tenant: Uuid,
    #[serde(default)]
    graph: Option<u32>,
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
    let map: std::collections::HashMap<u32, crate::state::GraphActivity> = state
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
    graph_id: u32,
}

/// Request a manual run of a tenant's graph. Async-job style: the engine applies the
/// graph's concurrency policy and runs on a background thread; we return `202 Accepted`
/// immediately and progress streams over `/events`.
async fn create_run(
    State(state): State<Arc<AppState>>,
    Json(body): Json<CreateRun>,
) -> impl IntoResponse {
    // Manual fire honors the graph's concurrency policy (spawn/queue/skip).
    let policy = crate::state::find_graph(&state.store, body.tenant_id, body.graph_id)
        .map(|g| g.concurrency.clone());
    let Some(policy) = policy else {
        return (StatusCode::NOT_FOUND, Json(json!({ "error": "no such graph" })));
    };
    engine::fire_graph(
        &state,
        body.tenant_id,
        body.graph_id,
        &policy,
        app_core::workflows::TriggerSource::Manual,
    );
    (StatusCode::ACCEPTED, Json(json!({ "graph_id": body.graph_id, "accepted": true })))
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

/// All persisted signal lines for one run of a tenant (`?tenant=<uuid>`).
async fn run_logs(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(run_id): axum::extract::Path<Uuid>,
    Query(q): Query<TenantQuery>,
) -> impl IntoResponse {
    let logs = app_core::workflows::repo::list_run_logs(&state.db, q.tenant, run_id);
    Json(logs)
}

/// SSE stream of engine events (`workflow-run`/`-node`/`-log`/`-print`/`-image`,
/// plus snapshot/alerts/devices pokes). Lossy on a slow client; final state is
/// always re-fetchable via the REST routes above.
async fn events(
    State(state): State<Arc<AppState>>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let rx = state.events.subscribe();
    let stream = BroadcastStream::new(rx).filter_map(|msg| match msg {
        Ok(line) => Some(Ok(Event::default().data(line))),
        Err(_) => None, // lagged: drop, keep streaming
    });
    Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
}
