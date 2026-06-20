//! Tenant-scoped floor plan (rooms + assigned cameras). Stateless: the client owns
//! the working copy and PUTs the whole array back (replace-all).
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::Json;
use axum::routing::get;
use axum::Router;
use serde::Deserialize;

use app_core::map::{self, MapDoc};
use app_core::presence::{self, Presence};

use super::{blocking, tenant_of, ApiErr};
use crate::auth::{self, UserClaims};
use crate::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/map", get(load).put(save))
        .route("/map/presence", get(recent_presence))
}

#[derive(Deserialize)]
struct PresenceQuery {
    limit: Option<i64>,
}

/// Recent presence events for the tenant (baseline for the map's live pulse).
async fn recent_presence(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
    Query(q): Query<PresenceQuery>,
) -> Result<Json<Vec<Presence>>, ApiErr> {
    auth::require(&c, "map.read")?;
    let tid = tenant_of(&c)?;
    let db = st.db.clone();
    let limit = q.limit.unwrap_or(100).clamp(1, 500);
    Ok(Json(blocking(move || presence::repo::load_recent(&db, tid, limit)).await?))
}

async fn load(UserClaims(c): UserClaims, State(st): State<AppState>) -> Result<Json<MapDoc>, ApiErr> {
    auth::require(&c, "map.read")?;
    let tid = tenant_of(&c)?;
    let db = st.db.clone();
    Ok(Json(blocking(move || map::repo::load_doc(&db, tid)).await?))
}

async fn save(UserClaims(c): UserClaims, State(st): State<AppState>, Json(doc): Json<MapDoc>) -> Result<StatusCode, ApiErr> {
    auth::require(&c, "map.write")?;
    let tid = tenant_of(&c)?;
    let db = st.db.clone();
    blocking(move || map::repo::save_doc(&db, tid, &doc)).await?;
    Ok(StatusCode::NO_CONTENT)
}
