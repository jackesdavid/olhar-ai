//! Edge appliance ("box") registry: a tenant admin creates boxes and gets a one-time
//! box token to put in the edge installer. The edge presents that token to
//! `/config/cameras` (see `http.rs`) and control returns only that box's tenant's
//! cameras. `/internal/box-upstream` lets the gateway resolve a tenant's live host.
use std::convert::Infallible;
use std::time::Duration;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::Json;
use axum::routing::get;
use axum::Router;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::{err, tenant_of, ApiErr};
use crate::auth::{self, UserClaims};
use crate::{store, AppState};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/boxes", get(list).post(create))
        .route("/boxes/:id", axum::routing::delete(remove))
        .route("/boxes/:id/restart", axum::routing::post(restart))
        .route("/boxes/:id/logs", get(box_logs))
        .route("/boxes/:id/logs/stream", get(box_logs_stream))
        .route("/internal/box-upstream", get(box_upstream))
}

fn db_err(e: sqlx::Error) -> ApiErr {
    log::error!("boxes db error: {e}");
    err(StatusCode::INTERNAL_SERVER_ERROR, "database error")
}

/// sha256(token) as hex — the deterministic form stored + looked up. Public so the
/// camera-config endpoint hashes the presented token the same way.
pub fn hash_token(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

/// A fresh high-entropy box token (`sbx_<32 hex><32 hex>`). Shown once.
fn new_token() -> String {
    format!("sbx_{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple())
}

async fn list(UserClaims(c): UserClaims, State(st): State<AppState>) -> Result<Json<Vec<store::BoxRow>>, ApiErr> {
    auth::require(&c, "devices.write")?;
    let tid = tenant_of(&c)?;
    Ok(Json(store::list_boxes(&st.pool, tid).await.map_err(db_err)?))
}

#[derive(Deserialize)]
struct CreateReq {
    name: String,
}

#[derive(Serialize)]
struct CreateResp {
    id: Uuid,
    /// The plaintext box token — shown ONCE; the installer needs it as BOX_TOKEN.
    token: String,
    /// A unique tailnet hostname for this box (`--hostname`).
    hostname: String,
    /// A freshly-minted, transparent Tailscale auth key (`--authkey`) when the
    /// enrollment broker is configured — so the tenant never logs into Tailscale.
    /// Null when not configured (the UI then asks for a key).
    tailscale_key: Option<String>,
}

async fn create(UserClaims(c): UserClaims, State(st): State<AppState>, Json(b): Json<CreateReq>) -> Result<Json<CreateResp>, ApiErr> {
    auth::require(&c, "devices.write")?;
    let tid = tenant_of(&c)?;
    let name = b.name.trim();
    if name.is_empty() {
        return Err(err(StatusCode::BAD_REQUEST, "box name required"));
    }
    let token = new_token();
    let id = store::create_box(&st.pool, tid, name, &hash_token(&token)).await.map_err(db_err)?;
    let hostname = format!("sentinel-{}", &id.to_string()[..8]);

    // Transparent enrollment: mint a Tailscale auth key for this box (best-effort —
    // if the broker isn't configured or the API fails, fall back to a manual key).
    let tailscale_key = match &st.tailscale {
        Some(ts) => match ts.mint_auth_key(&st.http, &format!("sentinel-edge {name}")).await {
            Ok(k) => Some(k),
            Err(e) => {
                log::warn!("tailscale key mint failed for box {id}: {e}");
                None
            }
        },
        None => None,
    };

    Ok(Json(CreateResp { id, token, hostname, tailscale_key }))
}

async fn remove(UserClaims(c): UserClaims, State(st): State<AppState>, Path(id): Path<Uuid>) -> Result<StatusCode, ApiErr> {
    auth::require(&c, "devices.write")?;
    let tid = tenant_of(&c)?;
    if store::delete_box(&st.pool, tid, id).await.map_err(db_err)? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(err(StatusCode::NOT_FOUND, "box not found"))
    }
}

/// Queue a restart for a box: the edge picks it up on its next poll and exits, so the
/// service manager relaunches it (re-running `tailscale up` + re-serving go2rtc).
async fn restart(UserClaims(c): UserClaims, State(st): State<AppState>, Path(id): Path<Uuid>) -> Result<StatusCode, ApiErr> {
    auth::require(&c, "devices.write")?;
    let tid = tenant_of(&c)?;
    if !store::box_belongs_to(&st.pool, tid, id).await.map_err(db_err)? {
        return Err(err(StatusCode::NOT_FOUND, "box not found"));
    }
    st.request_restart(id);
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize)]
struct UpstreamResp {
    /// The tenant's live go2rtc host (tailnet MagicDNS), or null if no box reported one.
    host: Option<String>,
}

/// The gateway calls this (forwarding the user's JWT) to resolve the caller's tenant's
/// live upstream. Any authenticated user with a tenant context may resolve their own.
async fn box_upstream(UserClaims(c): UserClaims, State(st): State<AppState>) -> Result<Json<UpstreamResp>, ApiErr> {
    let tid = tenant_of(&c)?;
    Ok(Json(UpstreamResp { host: store::box_host_for_tenant(&st.pool, tid).await.map_err(db_err)? }))
}

// ---- per-box logs -----------------------------------------------------------

#[derive(Deserialize)]
struct LogsQuery {
    limit: Option<i64>,
}

/// The recent persisted log tail for a box (oldest-first), so the viewer has history on open.
async fn box_logs(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
    Path(id): Path<Uuid>,
    Query(q): Query<LogsQuery>,
) -> Result<Json<Vec<store::BoxLogRow>>, ApiErr> {
    auth::require(&c, "devices.read")?;
    let tid = tenant_of(&c)?;
    if !store::box_belongs_to(&st.pool, tid, id).await.map_err(db_err)? {
        return Err(err(StatusCode::NOT_FOUND, "box not found"));
    }
    let limit = q.limit.unwrap_or(500).clamp(1, 5000);
    Ok(Json(store::list_box_logs(&st.pool, tid, id, limit).await.map_err(db_err)?))
}

#[derive(Deserialize)]
struct TokenQuery {
    token: String,
}

/// Live SSE tail of a box's logs, scoped to the caller's tenant + this box. JWT rides the
/// `token` query param (EventSource can't set headers). Subscribes to the control-wide
/// `log_bus` (fed by the edge's /config/logs pushes) and forwards only matching lines.
async fn box_logs_stream(
    State(st): State<AppState>,
    Path(id): Path<Uuid>,
    Query(q): Query<TokenQuery>,
) -> Result<Sse<impl futures::Stream<Item = Result<Event, Infallible>>>, ApiErr> {
    let claims = auth::verify(&st.jwt_secret, &q.token).map_err(|e| err(StatusCode::UNAUTHORIZED, e))?;
    auth::require(&claims, "devices.read")?;
    let tid = tenant_of(&claims)?;
    if !store::box_belongs_to(&st.pool, tid, id).await.map_err(db_err)? {
        return Err(err(StatusCode::NOT_FOUND, "box not found"));
    }
    let tid_s = tid.to_string();
    let box_s = id.to_string();

    let mut rx = st.log_bus.subscribe();
    let (tx, out) = tokio::sync::mpsc::channel::<Result<Event, Infallible>>(256);
    tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(line) => {
                    let matches = serde_json::from_str::<Value>(&line)
                        .ok()
                        .map(|v| {
                            v.get("tenant").and_then(|t| t.as_str()) == Some(tid_s.as_str())
                                && v.get("box").and_then(|b| b.as_str()) == Some(box_s.as_str())
                        })
                        .unwrap_or(false);
                    if matches && tx.send(Ok(Event::default().data(line))).await.is_err() {
                        return; // browser disconnected
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue, // dropped some — keep going
                Err(_) => return,                                                      // sender gone
            }
        }
    });

    let stream = tokio_stream::wrappers::ReceiverStream::new(out);
    Ok(Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15))))
}
