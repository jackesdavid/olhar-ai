//! Faces labeling view (#119) — browse/cluster the tenant's UNKNOWN captured faces and promote a
//! cluster into a person in one action. Reuses `persons.read`/`persons.write` scopes (labeling faces
//! is a persons operation). Mirrors `dictionaries.rs` for CRUD and `run_frame` for the image proxy.
use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Json, Response};
use axum::routing::{get, post};
use axum::Router;
use serde::Deserialize;
use serde_json::json;

use app_core::faces::{self, Face, FaceCluster};
use app_core::persons;

use super::{blocking, err, tenant_of, ApiErr};
use crate::auth::{self, UserClaims};
use crate::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/faces", get(list))
        .route("/faces/clusters", get(clusters))
        .route("/faces/layout", get(layout))
        .route("/faces/:id/image", get(face_image))
        .route("/faces/label", post(label))
        .route("/faces/reassign", post(reassign))
        .route("/faces/ignore", post(ignore))
        .route("/faces/delete", post(delete_faces))
}

async fn list(UserClaims(c): UserClaims, State(st): State<AppState>) -> Result<Json<Vec<Face>>, ApiErr> {
    auth::require(&c, "persons.read")?;
    let tid = tenant_of(&c)?;
    let db = st.db.clone();
    Ok(Json(blocking(move || faces::list(&db, tid, 1000)).await?))
}

#[derive(Deserialize)]
struct ClusterQuery {
    /// Grouping distance (cosine) — tunable live from the screen; defaults to `faces::CLUSTER_EPS`.
    eps: Option<f32>,
    /// Which faces to show: "unknown" (default) · "known" · "all".
    scope: Option<String>,
}

/// The triage feed: unknown faces grouped into clusters, each with an auto-suggested person.
async fn clusters(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
    Query(q): Query<ClusterQuery>,
) -> Result<Json<Vec<FaceCluster>>, ApiErr> {
    auth::require(&c, "persons.read")?;
    let tid = tenant_of(&c)?;
    let db = st.db.clone();
    let eps = q.eps.unwrap_or(faces::CLUSTER_EPS);
    Ok(Json(blocking(move || faces::cluster_unlabeled(&db, tid, eps)).await?))
}

/// The 2D canvas layout: unknown faces grouped into islands positioned by similarity (#119 Phase 1).
async fn layout(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
    Query(q): Query<ClusterQuery>,
) -> Result<Json<faces::CanvasLayout>, ApiErr> {
    auth::require(&c, "persons.read")?;
    let tid = tenant_of(&c)?;
    let db = st.db.clone();
    let eps = q.eps.unwrap_or(faces::CLUSTER_EPS);
    let scope = faces::FaceScope::parse(q.scope.as_deref().unwrap_or("unknown"));
    Ok(Json(blocking(move || faces::layout_faces(&db, tid, eps, scope)).await?))
}

#[derive(Deserialize)]
struct ImageQuery {
    token: String,
    /// `?box=1` → full frame with the detection rectangle drawn (context); default = crop to face.
    #[serde(rename = "box", default)]
    show_box: Option<u8>,
}

/// Stream a face image by id. The stored object is the whole FRAME; by default we crop it to the
/// face bbox (so `<img>` shows the face), or with `?box=1` draw the detection rectangle on the full
/// frame. Token-in-query (like `run_frame`). Tenant-gated on the `faces` row; 404 for a foreign
/// tenant / no stored image.
async fn face_image(
    State(st): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<ImageQuery>,
) -> Result<Response, ApiErr> {
    let claims = auth::verify(&st.jwt_secret, &q.token).map_err(|e| err(StatusCode::UNAUTHORIZED, e))?;
    auth::require(&claims, "persons.read")?;
    let tid = tenant_of(&claims)?;
    let db = st.db.clone();
    let (key, bbox) = match blocking(move || faces::crop_info(&db, tid, &id)).await? {
        Some((Some(k), bb)) => (k, bb),
        _ => return Ok(StatusCode::NOT_FOUND.into_response()),
    };
    let bytes = blocking(move || app_core::media::get(&key))
        .await?
        .map_err(|e| err(StatusCode::NOT_FOUND, e))?;
    let draw_box = q.show_box.unwrap_or(0) != 0;
    let out = blocking(move || match bbox {
        Some(bb) => faces::render_face_image(&bytes, bb, draw_box).unwrap_or(bytes),
        None => bytes,
    })
    .await?;
    Ok(([(header::CONTENT_TYPE, "image/jpeg")], out).into_response())
}

#[derive(Deserialize)]
struct LabelReq {
    face_ids: Vec<String>,
    #[serde(default)]
    person_id: Option<String>,
    #[serde(default)]
    new_person_name: Option<String>,
}

/// The core bulk action: label a set of faces (a whole cluster) as a person — an existing one, or a
/// brand-new one created from `new_person_name`. Copies embeddings into `person_embeddings` (approved)
/// + stamps `faces.person_id`, one transaction. Returns `{ person_id, labeled }`.
async fn label(UserClaims(c): UserClaims, State(st): State<AppState>, Json(b): Json<LabelReq>) -> Result<Json<serde_json::Value>, ApiErr> {
    auth::require(&c, "persons.write")?;
    let tid = tenant_of(&c)?;
    if b.face_ids.is_empty() {
        return Err(err(StatusCode::BAD_REQUEST, "no faces to label"));
    }
    let db = st.db.clone();
    let (person_id, labeled) = blocking(move || -> Result<(String, u64), String> {
        // Resolve the target person (existing id) or create a new one.
        let person_id = match b.person_id.filter(|s| !s.trim().is_empty()) {
            Some(pid) => {
                if persons::find(&persons::repo::load(&db, tid), &pid).is_none() {
                    return Err("Person not found.".into());
                }
                pid
            }
            None => {
                let name = b.new_person_name.unwrap_or_default().trim().to_string();
                if name.is_empty() {
                    return Err("A person or a new name is required.".into());
                }
                let mut people = persons::repo::load(&db, tid);
                let id = uuid::Uuid::new_v4().to_string();
                people.push(persons::Person {
                    id: id.clone(),
                    name,
                    parent_id: None,
                    recipient_label: None,
                    permissions: vec![],
                });
                persons::repo::save(&db, tid, &people);
                id
            }
        };
        let labeled = faces::label_faces(&db, tid, &b.face_ids, &person_id)?;
        Ok((person_id, labeled))
    })
    .await?
    .map_err(|e| err(StatusCode::BAD_REQUEST, e))?;
    Ok(Json(json!({ "person_id": person_id, "labeled": labeled })))
}

#[derive(Deserialize)]
struct ReassignReq {
    face_id: String,
    #[serde(default)]
    person_id: Option<String>, // None → back to unknown
}

/// Move a single mis-clustered face to another person (or back to unknown with `person_id=null`).
async fn reassign(UserClaims(c): UserClaims, State(st): State<AppState>, Json(b): Json<ReassignReq>) -> Result<StatusCode, ApiErr> {
    auth::require(&c, "persons.write")?;
    let tid = tenant_of(&c)?;
    let db = st.db.clone();
    blocking(move || faces::set_person(&db, tid, &[b.face_id], b.person_id.as_deref()))
        .await?
        .map_err(|e| err(StatusCode::BAD_REQUEST, e))?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct IgnoreReq {
    face_ids: Vec<String>,
    #[serde(default)]
    ignored: Option<bool>, // default true
}

/// Soft-dismiss (or restore) junk crops — they leave the triage queue but the row + crop stay.
async fn ignore(UserClaims(c): UserClaims, State(st): State<AppState>, Json(b): Json<IgnoreReq>) -> Result<StatusCode, ApiErr> {
    auth::require(&c, "persons.write")?;
    let tid = tenant_of(&c)?;
    let db = st.db.clone();
    let ig = b.ignored.unwrap_or(true);
    blocking(move || faces::set_ignored(&db, tid, &b.face_ids, ig))
        .await?
        .map_err(|e| err(StatusCode::BAD_REQUEST, e))?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct DeleteReq {
    face_ids: Vec<String>,
}

/// HARD-delete faces — remove the rows AND purge their MinIO crops. For false positives / far,
/// junk detections you never want back (unlike `ignore`, which is recoverable). Returns `{deleted}`.
async fn delete_faces(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
    Json(b): Json<DeleteReq>,
) -> Result<Json<serde_json::Value>, ApiErr> {
    auth::require(&c, "persons.write")?;
    let tid = tenant_of(&c)?;
    let db = st.db.clone();
    let n = b.face_ids.len();
    let keys = blocking(move || faces::delete(&db, tid, &b.face_ids))
        .await?
        .map_err(|e| err(StatusCode::BAD_REQUEST, e))?;
    // Purge the crops off the object store (best-effort — a missing key isn't fatal).
    blocking(move || {
        for k in keys {
            if let Err(e) = app_core::media::delete(&k) {
                log::warn!("faces::delete_faces: media purge failed for {k}: {e}");
            }
        }
    })
    .await?;
    Ok(Json(json!({ "deleted": n })))
}
