//! The cloud→edge **SSE command stream** (card #144) — control's side of the outbound transport
//! that replaces the tailnet for commanding an edge.
//!
//! An edge opens one long-lived `GET /config/edge/stream` (box token). This module holds the
//! per-pod registry of those live streams (`box_id → sender`), pushes queued commands down the
//! right one, and accepts the edge's result POST. The durable queue is [`app_core::edge_commands`];
//! here is only the live-connection half.
//!
//! **Multi-pod.** The stream pins a box to whichever pod the load balancer sent it to. A command
//! can be enqueued on any pod/service, so delivery is decoupled: the producer NOTIFYs, and the pod
//! that *holds* the box's stream (this registry) is the one that claims and delivers — the
//! connection selects the deliverer, not the caller. Mirrors [`crate::clip_purge`]'s LISTEN + poll
//! consumer.

use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Json, Response};
use sqlx::postgres::PgListener;
use sqlx::PgPool;
use tokio_stream::wrappers::ReceiverStream;
use uuid::Uuid;

use crate::http::box_identity;
use crate::AppState;
use app_core::edge_commands;

/// One live edge stream: a unique connection id (to guard against a reconnect's cleanup removing the
/// newer connection) and the channel that feeds its SSE response.
type Conn = (Uuid, tokio::sync::mpsc::Sender<Result<Event, Infallible>>);
/// Per-pod registry of the edge streams THIS pod holds.
pub type EdgeStreams = Arc<Mutex<HashMap<Uuid, Conn>>>;

/// Fresh registry (one per pod, shared into [`AppState`] and the delivery task).
pub fn new_registry() -> EdgeStreams {
    Arc::new(Mutex::new(HashMap::new()))
}

/// This pod's name — used only for observability (`tenant_boxes.sse_pod`), not for routing.
pub fn pod_name() -> String {
    std::env::var("POD_NAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "control".to_string())
}

/// Fallback poll — catches a missed NOTIFY and any command that became due while nothing woke us.
const POLL: Duration = Duration::from_secs(15);
/// Refresh `sse_last_seen` this often while a stream is up, so `has_live_session` stays true.
const HEARTBEAT: Duration = Duration::from_secs(20);
/// Commands claimed per box per pass.
const BATCH: i64 = 20;

// ---- delivery task (one per pod) -------------------------------------------

/// Spawn the per-pod delivery loop: LISTEN `edge_commands`, and on each wake (or poll) push any
/// queued commands to the boxes THIS pod holds. Reconnects the listener on drop; the poll drains
/// meanwhile. Mirrors [`crate::clip_purge`].
pub fn spawn_delivery(pool: PgPool, registry: EdgeStreams) {
    tokio::spawn(async move {
        loop {
            let mut listener = match connect_listener(&pool).await {
                Ok(l) => l,
                Err(e) => {
                    log::warn!("[edge-stream] listen unavailable ({e}); polling only");
                    deliver_all(&pool, &registry).await;
                    tokio::time::sleep(POLL).await;
                    continue;
                }
            };
            loop {
                deliver_all(&pool, &registry).await;
                match tokio::time::timeout(POLL, listener.recv()).await {
                    Ok(Ok(notif)) => {
                        // Payload is the target box_id — deliver just to it if we hold it.
                        if let Ok(box_id) = notif.payload().parse::<Uuid>() {
                            deliver_box(&pool, &registry, box_id).await;
                        }
                    }
                    Ok(Err(e)) => {
                        log::warn!("[edge-stream] listener dropped ({e}); reconnecting");
                        break;
                    }
                    Err(_) => {} // poll timeout — loop and drain
                }
            }
        }
    });
}

async fn connect_listener(pool: &PgPool) -> Result<PgListener, sqlx::Error> {
    let mut l = PgListener::connect_with(pool).await?;
    l.listen(edge_commands::CHANNEL).await?;
    Ok(l)
}

/// Deliver queued commands to every box this pod currently holds.
async fn deliver_all(pool: &PgPool, registry: &EdgeStreams) {
    let boxes: Vec<Uuid> = registry.lock().unwrap().keys().copied().collect();
    for box_id in boxes {
        deliver_box(pool, registry, box_id).await;
    }
}

/// Claim and push any deliverable commands for one box down its stream. A send failure means the
/// edge just disconnected — release the command so it's redelivered when the box comes back.
pub async fn deliver_box(pool: &PgPool, registry: &EdgeStreams, box_id: Uuid) {
    let tx = match registry.lock().unwrap().get(&box_id) {
        Some((_, tx)) => tx.clone(),
        None => return, // not ours
    };
    let cmds = match edge_commands::claim_for_box(pool, box_id, BATCH).await {
        Ok(c) => c,
        Err(e) => {
            log::warn!("[edge-stream] claim for {box_id} failed: {e}");
            return;
        }
    };
    for c in cmds {
        let payload = serde_json::json!({ "v": 1, "id": c.id, "cmd": c.cmd, "args": c.args });
        let ev = match Event::default()
            .event("command")
            .id(c.id.to_string())
            .json_data(&payload)
        {
            Ok(ev) => ev,
            Err(e) => {
                log::error!("[edge-stream] encode command {}: {e}", c.id);
                continue;
            }
        };
        if tx.send(Ok(ev)).await.is_err() {
            // Edge dropped between claim and send — hand it back for redelivery.
            let _ = edge_commands::release(pool, c.id, 2).await;
            return;
        }
    }
}

// ---- the edge's SSE stream (GET /config/edge/stream) ------------------------

/// The edge holds this open; commands ride down as `event: command` frames. Box-token auth. On
/// connect we register the stream, mark the session, and drain anything already queued; a companion
/// task heartbeats `sse_last_seen` and, on disconnect, unregisters + clears the session.
pub async fn edge_stream(State(st): State<AppState>, headers: HeaderMap) -> Response {
    let (_tenant, box_id) = match box_identity(&st, &headers).await {
        Ok(Some(pair)) => pair,
        Ok(None) => return (StatusCode::UNAUTHORIZED, "box token required").into_response(),
        Err(resp) => return resp,
    };
    let pod = pod_name();
    let conn_id = Uuid::new_v4();
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<Event, Infallible>>(256);

    // Register (replacing any stale entry for this box — a reconnect wins).
    st.edge_streams
        .lock()
        .unwrap()
        .insert(box_id, (conn_id, tx.clone()));
    let _ = edge_commands::mark_session(&st.pool, box_id, &pod).await;
    log::info!("[edge-stream] box {box_id} connected (pod {pod})");

    // Drain whatever is already queued for this box, now.
    deliver_box(&st.pool, &st.edge_streams, box_id).await;

    // Companion task: heartbeat the session while up; on disconnect (receiver dropped → tx closed),
    // unregister *only if still ours* and clear the session.
    {
        let pool = st.pool.clone();
        let registry = st.edge_streams.clone();
        let pod = pod.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = tx.closed() => break,
                    _ = tokio::time::sleep(HEARTBEAT) => {
                        let _ = edge_commands::mark_session(&pool, box_id, &pod).await;
                    }
                }
            }
            // Cleanup — but don't evict a newer reconnect that replaced us.
            {
                let mut map = registry.lock().unwrap();
                if map
                    .get(&box_id)
                    .map(|(cid, _)| *cid == conn_id)
                    .unwrap_or(false)
                {
                    map.remove(&box_id);
                }
            }
            let _ = edge_commands::clear_session(&pool, box_id, &pod).await;
            log::info!("[edge-stream] box {box_id} disconnected");
        });
    }

    Sse::new(ReceiverStream::new(rx))
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
        .into_response()
}

// ---- the edge's result POST (POST /config/edge/command-result) --------------

#[derive(serde::Deserialize)]
pub struct CommandResultIn {
    pub id: Uuid,
    #[serde(default)]
    pub ok: bool,
    #[serde(default)]
    pub result: Option<serde_json::Value>,
    #[serde(default)]
    pub error: Option<String>,
}

/// The edge reports a command's outcome. Box-token auth; fills the row and wakes the waiting caller.
pub async fn command_result(
    State(st): State<AppState>,
    headers: HeaderMap,
    body: Option<Json<CommandResultIn>>,
) -> Response {
    match box_identity(&st, &headers).await {
        Ok(Some(_)) => {}
        Ok(None) => return (StatusCode::UNAUTHORIZED, "box token required").into_response(),
        Err(resp) => return resp,
    }
    let Some(Json(b)) = body else {
        return (StatusCode::BAD_REQUEST, "result body required").into_response();
    };
    match edge_commands::finish(&st.pool, b.id, b.ok, b.result, b.error.as_deref()).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => {
            log::error!("[edge-stream] finish {}: {e}", b.id);
            (StatusCode::INTERNAL_SERVER_ERROR, "result save failed").into_response()
        }
    }
}
