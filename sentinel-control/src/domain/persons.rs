//! Tenant-scoped people: the hierarchy (recipients + permissions) and per-person
//! face embeddings (enroll / approve / recognize). Face embedding runs via
//! `app_core::agent::face` (the GPU sidecar) on a blocking task.
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Json;
use axum::routing::{get, post};
use axum::Router;
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use serde::{Deserialize, Serialize};

use app_core::persons::{self, Person, PersonEmbedding, RecognitionResult};

use super::{blocking, err, tenant_of, ApiErr};
use crate::auth::{self, UserClaims};
use crate::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/persons", get(list).put(upsert))
        .route("/persons/:id", axum::routing::delete(remove))
        .route("/persons/enrolled", get(enrolled))
        .route("/persons/permission-kinds", get(permission_kinds))
        .route("/persons/recognize", post(recognize))
        .route("/persons/:id/embeddings", get(list_embeddings).post(enroll))
        .route("/persons/embeddings/:eid/approve", post(approve_embedding))
        .route("/persons/embeddings/:eid", axum::routing::delete(delete_embedding))
}

async fn list(UserClaims(c): UserClaims, State(st): State<AppState>) -> Result<Json<Vec<Person>>, ApiErr> {
    auth::require(&c, "persons.read")?;
    let tid = tenant_of(&c)?;
    let db = st.db.clone();
    Ok(Json(blocking(move || persons::repo::load(&db, tid)).await?))
}

/// The permission actions a superior can hold over its subtree (UI toggles).
async fn permission_kinds(UserClaims(c): UserClaims) -> Result<Json<Vec<String>>, ApiErr> {
    auth::require(&c, "persons.read")?;
    Ok(Json(persons::PERMISSIONS.iter().map(|s| s.to_string()).collect()))
}

/// Upsert a person by `id` (validates name + guards the hierarchy against cycles).
async fn upsert(UserClaims(c): UserClaims, State(st): State<AppState>, Json(mut person): Json<Person>) -> Result<StatusCode, ApiErr> {
    auth::require(&c, "persons.write")?;
    let tid = tenant_of(&c)?;
    person.name = person.name.trim().to_string();
    if person.id.trim().is_empty() {
        return Err(err(StatusCode::BAD_REQUEST, "Person id is required."));
    }
    if person.name.is_empty() {
        return Err(err(StatusCode::BAD_REQUEST, "Name cannot be empty."));
    }
    person.recipient_label = person.recipient_label.filter(|r| !r.trim().is_empty());
    person.parent_id = person.parent_id.filter(|p| !p.trim().is_empty());
    person.permissions.retain(|a| persons::PERMISSIONS.contains(&a.as_str()));

    let db = st.db.clone();
    blocking(move || {
        let mut people = persons::repo::load(&db, tid);
        if let Some(parent) = person.parent_id.as_deref() {
            if persons::find(&people, parent).is_none() {
                return Err("Selected parent no longer exists.".to_string());
            }
            if persons::would_cycle(&people, &person.id, parent) {
                return Err("That parent would create a cycle in the hierarchy.".to_string());
            }
        }
        if let Some(existing) = people.iter_mut().find(|p| p.id == person.id) {
            *existing = person;
        } else {
            people.push(person);
        }
        persons::repo::save(&db, tid, &people);
        Ok(())
    })
    .await?
    .map_err(|e| err(StatusCode::BAD_REQUEST, e))?;
    Ok(StatusCode::NO_CONTENT)
}

/// Remove a person and detach any children that pointed to them.
async fn remove(UserClaims(c): UserClaims, State(st): State<AppState>, Path(id): Path<String>) -> Result<StatusCode, ApiErr> {
    auth::require(&c, "persons.write")?;
    let tid = tenant_of(&c)?;
    let db = st.db.clone();
    blocking(move || {
        let mut people = persons::repo::load(&db, tid);
        let before = people.len();
        people.retain(|p| p.id != id);
        if people.len() != before {
            for child in people.iter_mut().filter(|p| p.parent_id.as_deref() == Some(id.as_str())) {
                child.parent_id = None;
            }
            persons::repo::save(&db, tid, &people);
        }
    })
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Ids of people with ≥1 approved embedding — the "enrolled" badge.
async fn enrolled(UserClaims(c): UserClaims, State(st): State<AppState>) -> Result<Json<Vec<String>>, ApiErr> {
    auth::require(&c, "persons.read")?;
    let tid = tenant_of(&c)?;
    let db = st.db.clone();
    Ok(Json(blocking(move || persons::repo::enrolled_person_ids(&db, tid)).await?))
}

async fn list_embeddings(UserClaims(c): UserClaims, State(st): State<AppState>, Path(id): Path<String>) -> Result<Json<Vec<PersonEmbedding>>, ApiErr> {
    auth::require(&c, "persons.read")?;
    let tid = tenant_of(&c)?;
    let db = st.db.clone();
    Ok(Json(blocking(move || persons::repo::list_embeddings(&db, tid, &id)).await?))
}

#[derive(Deserialize)]
struct EnrollReq {
    image: String,
    source: String,
    approved: bool,
}

/// Enroll one face: embed `image` (base64, optionally a data URL) and insert it.
async fn enroll(UserClaims(c): UserClaims, State(st): State<AppState>, Path(id): Path<String>, Json(b): Json<EnrollReq>) -> Result<Json<String>, ApiErr> {
    auth::require(&c, "persons.write")?;
    let tid = tenant_of(&c)?;
    let bytes = decode_image(&b.image)?;
    let db = st.db.clone();
    let new_id = blocking(move || {
        if persons::find(&persons::repo::load(&db, tid), &id).is_none() {
            return Err("Person not found.".to_string());
        }
        let embedding = app_core::agent::face::embed(&bytes)?;
        persons::repo::insert_embedding(&db, tid, &id, &embedding, b.approved, &b.source)
    })
    .await?
    .map_err(|e| err(StatusCode::BAD_REQUEST, e))?;
    Ok(Json(new_id))
}

async fn approve_embedding(UserClaims(c): UserClaims, State(st): State<AppState>, Path(eid): Path<String>) -> Result<StatusCode, ApiErr> {
    auth::require(&c, "persons.write")?;
    let tid = tenant_of(&c)?;
    let db = st.db.clone();
    blocking(move || persons::repo::approve_embedding(&db, tid, &eid))
        .await?
        .map_err(|e| err(StatusCode::BAD_REQUEST, e))?;
    Ok(StatusCode::NO_CONTENT)
}

async fn delete_embedding(UserClaims(c): UserClaims, State(st): State<AppState>, Path(eid): Path<String>) -> Result<StatusCode, ApiErr> {
    auth::require(&c, "persons.write")?;
    let tid = tenant_of(&c)?;
    let db = st.db.clone();
    blocking(move || persons::repo::delete_embedding(&db, tid, &eid))
        .await?
        .map_err(|e| err(StatusCode::BAD_REQUEST, e))?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct RecognizeReq {
    image: String,
    max_distance: Option<f32>,
}

#[derive(Serialize)]
struct RecognizeResp {
    query: Vec<f32>,
    #[serde(rename = "match")]
    matched: Option<RecognitionResult>,
}

/// Recognise a face: embed `image` and match it against the tenant's approved set.
async fn recognize(UserClaims(c): UserClaims, State(st): State<AppState>, Json(b): Json<RecognizeReq>) -> Result<Json<RecognizeResp>, ApiErr> {
    auth::require(&c, "persons.read")?;
    let tid = tenant_of(&c)?;
    let bytes = decode_image(&b.image)?;
    let db = st.db.clone();
    let resp = blocking(move || {
        let query = app_core::agent::face::embed(&bytes)?;
        let matches = persons::repo::recognize(&db, tid, &query, 5);
        let matched = persons::best_match(&matches, b.max_distance.unwrap_or(0.4));
        Ok::<_, String>(RecognizeResp { query, matched })
    })
    .await?
    .map_err(|e| err(StatusCode::BAD_REQUEST, e))?;
    Ok(Json(resp))
}

/// Decode a base64 image (optionally a `data:…;base64,` URL) to bytes.
fn decode_image(image: &str) -> Result<Vec<u8>, ApiErr> {
    let b64 = image.rsplit(',').next().unwrap_or(image);
    B64.decode(b64.trim()).map_err(|e| err(StatusCode::BAD_REQUEST, format!("decode image: {e}")))
}
