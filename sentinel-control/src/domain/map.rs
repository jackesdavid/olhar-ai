//! Tenant-scoped floor plan (rooms + assigned cameras). Stateless: the client owns
//! the working copy and PUTs the whole array back (replace-all).
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Json;
use axum::routing::get;
use axum::Router;

use app_core::map::{self, MapRoom};

use super::{blocking, tenant_of, ApiErr};
use crate::auth::{self, UserClaims};
use crate::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/map", get(load).put(save))
}

async fn load(UserClaims(c): UserClaims, State(st): State<AppState>) -> Result<Json<Vec<MapRoom>>, ApiErr> {
    auth::require(&c, "map.read")?;
    let tid = tenant_of(&c)?;
    let db = st.db.clone();
    Ok(Json(blocking(move || map::repo::load(&db, tid)).await?))
}

async fn save(UserClaims(c): UserClaims, State(st): State<AppState>, Json(rooms): Json<Vec<MapRoom>>) -> Result<StatusCode, ApiErr> {
    auth::require(&c, "map.write")?;
    let tid = tenant_of(&c)?;
    let db = st.db.clone();
    blocking(move || map::repo::save(&db, tid, &rooms)).await?;
    Ok(StatusCode::NO_CONTENT)
}
