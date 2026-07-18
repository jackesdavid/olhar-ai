//! Tenant-scoped Dictionaries (#113) — named, typed-column tables. CRUD persisted to the shared
//! Postgres (`dictionaries`), where `sentinel-flow` reads + mutates them at run time. Delete is
//! guarded while a workflow variable still references the dictionary (by id) so a live graph can't
//! be silently orphaned.
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Json;
use axum::routing::get;
use axum::Router;
use serde::Deserialize;
use uuid::Uuid;

use app_core::dictionaries::{self, Column, Dictionary};
use app_core::workflows::{self, domain::PortType};

use super::{blocking, err, tenant_of, ApiErr};
use crate::auth::{self, UserClaims};
use crate::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/dictionaries", get(list).post(create))
        .route(
            "/dictionaries/:id",
            get(get_one).put(update).delete(remove),
        )
}

#[derive(Deserialize)]
struct SaveReq {
    name: String,
    #[serde(default)]
    columns: Vec<Column>,
    #[serde(default)]
    rows: Vec<Vec<dictionaries::Cell>>,
}

async fn list(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
) -> Result<Json<Vec<Dictionary>>, ApiErr> {
    auth::require(&c, "workflows.read")?;
    let tid = tenant_of(&c)?;
    let db = st.db.clone();
    let out = blocking(move || dictionaries::list(&db, tid)).await?;
    Ok(Json(out))
}

async fn get_one(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Dictionary>, ApiErr> {
    auth::require(&c, "workflows.read")?;
    let tid = tenant_of(&c)?;
    let db = st.db.clone();
    let out = blocking(move || dictionaries::get(&db, tid, id))
        .await?
        .ok_or_else(|| err(StatusCode::NOT_FOUND, "dictionary not found"))?;
    Ok(Json(out))
}

async fn create(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
    Json(b): Json<SaveReq>,
) -> Result<Json<Uuid>, ApiErr> {
    auth::require(&c, "workflows.write")?;
    let tid = tenant_of(&c)?;
    let db = st.db.clone();
    let id = blocking(move || dictionaries::create(&db, tid, &b.name, &b.columns, &b.rows))
        .await?
        .map_err(|e| err(StatusCode::BAD_REQUEST, e))?;
    Ok(Json(id))
}

async fn update(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
    Path(id): Path<Uuid>,
    Json(b): Json<SaveReq>,
) -> Result<Json<Dictionary>, ApiErr> {
    auth::require(&c, "workflows.write")?;
    let tid = tenant_of(&c)?;
    let db = st.db.clone();
    let out = blocking(move || dictionaries::update(&db, tid, id, &b.name, &b.columns, &b.rows))
        .await?
        .map_err(|e| err(StatusCode::BAD_REQUEST, e))?;
    Ok(Json(out))
}

async fn remove(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiErr> {
    auth::require(&c, "workflows.write")?;
    let tid = tenant_of(&c)?;
    let db = st.db.clone();
    // Guard: refuse to delete while a workflow variable still references this dictionary by id.
    let users = blocking(move || referencing_workflows(&db, tid, id)).await?;
    if !users.is_empty() {
        return Err(err(
            StatusCode::CONFLICT,
            format!("In use by {}: {}", users.len(), users.join(", ")),
        ));
    }
    let db = st.db.clone();
    blocking(move || dictionaries::delete(&db, tid, id))
        .await?
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(StatusCode::NO_CONTENT)
}

/// Names of the tenant's workflows that reference `dict_id` through a `Table`-typed variable whose
/// default is the dictionary's id.
fn referencing_workflows(db: &app_core::db::Db, tid: Uuid, dict_id: Uuid) -> Vec<String> {
    let needle = dict_id.to_string();
    workflows::repo::load(db, tid)
        .into_iter()
        .filter(|g| {
            g.variables.iter().any(|v| {
                v.ty == PortType::Table && v.default.as_deref() == Some(needle.as_str())
            })
        })
        .map(|g| g.name)
        .collect()
}
