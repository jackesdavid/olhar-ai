//! Tenant-scoped floor plan (rooms + assigned cameras). Stateless: the client owns
//! the working copy and PUTs the whole array back (replace-all).
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Json;
use axum::routing::get;
use axum::Router;

use app_core::map::{self, MapDoc};

use super::{blocking, tenant_of, ApiErr};
use crate::auth::{self, UserClaims};
use crate::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/map", get(load).put(save))
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
