//! The REST + SSE control plane. The UI becomes a client: it lists graphs, polls
//! run state, asks for a manual run (`POST /runs` → 202, async-job style), and
//! subscribes to the live event stream. The engine remains authoritative; SSE is an
//! optimization layered over re-fetchable REST state.
use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Json};
use axum::routing::{get, post};
use axum::Router;
use futures::Stream;
use serde::Deserialize;
use serde_json::json;
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
        .route("/events", get(events))
        .layer(CorsLayer::permissive())
        .with_state(state)
}

/// Liveness — and a count of loaded graphs so a probe can confirm hydration.
async fn health(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let graphs = state.store.lock().unwrap().graphs.len();
    Json(json!({ "status": "ok", "service": "sentinel-flow", "graphs": graphs }))
}

/// All named graphs (triggers, concurrency, nodes, typed edges) — the working set.
async fn graphs(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let graphs = state.store.lock().unwrap().graphs.clone();
    Json(graphs)
}

/// Reload the graph set from Postgres (the UI edits land there out-of-band).
async fn reload_graphs(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    state.reload_graphs();
    let n = state.store.lock().unwrap().graphs.len();
    log::info!("graphs reloaded on request — {n} graph(s)");
    (StatusCode::OK, Json(json!({ "reloaded": n })))
}

/// Live run state per graph id (`{ running, current }`).
async fn run_states(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let map = state.activity.lock().unwrap().clone();
    Json(map)
}

#[derive(Deserialize)]
struct CreateRun {
    graph_id: u32,
}

/// Request a manual run of a graph. Async-job style: the engine applies the graph's
/// concurrency policy and runs on a background thread; we return `202 Accepted`
/// immediately and progress streams over `/events`.
async fn create_run(
    State(state): State<Arc<AppState>>,
    Json(body): Json<CreateRun>,
) -> impl IntoResponse {
    let exists = state.store.lock().unwrap().graph(body.graph_id).is_some();
    if !exists {
        return (StatusCode::NOT_FOUND, Json(json!({ "error": "no such graph" })));
    }
    // Manual fire honors the graph's concurrency policy (spawn/queue/skip).
    let policy = {
        let s = state.store.lock().unwrap();
        s.graph(body.graph_id).map(|g| g.concurrency.clone())
    };
    if let Some(policy) = policy {
        engine::fire_graph(&state, body.graph_id, &policy);
    }
    (StatusCode::ACCEPTED, Json(json!({ "graph_id": body.graph_id, "accepted": true })))
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
