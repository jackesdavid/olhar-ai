//! Edge appliance ("box") registry: a tenant admin creates boxes and gets a one-time
//! box token to put in the edge installer. The edge presents that token to
//! `/config/cameras` (see `http.rs`) and control returns only that box's tenant's
//! cameras. `/internal/box-upstream` lets the gateway resolve a tenant's live host.
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Json;
use axum::routing::get;
use axum::Router;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::{err, tenant_of, ApiErr};
use crate::auth::{self, UserClaims};
use crate::{store, AppState};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/boxes", get(list).post(create))
        .route("/boxes/:id", axum::routing::delete(remove))
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
    Ok(Json(CreateResp { id, token }))
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
