//! People — Tauri adapter for the persons slice: the person hierarchy (recipients +
//! permissions) and per-person face embeddings (enroll/recognize). The domain lives
//! in `app_core::persons`; persistence in `app_core::persons::repo`; face embedding
//! in `app_core::agent::face`.
use std::sync::{Arc, Mutex};

use base64::{engine::general_purpose::STANDARD as B64, Engine};
use serde::Serialize;
use tauri::Manager;

use app_core::db::Db;
use app_core::persons::{self, Person, PersonEmbedding, PersonStore, RecognitionResult};

use crate::db_ref;

// ---- lifecycle (called by lib.rs setup / try_connect_db) --------------------

/// Manage an empty person store BEFORE the DB connect so [`hydrate`] can fill it.
pub fn manage_empty_store(app: &tauri::AppHandle) {
    let store: PersonStore = Arc::new(Mutex::new(Vec::new()));
    app.manage(store);
}

/// Hydrate the in-memory person store from Postgres.
pub fn hydrate(app: &tauri::AppHandle, db: &Db) {
    let people = persons::repo::load(db);
    log::info!("Persons loaded — {} person(s)", people.len());
    *app.state::<PersonStore>().lock().unwrap() = people;
}

/// Persist the full person list to Postgres (upsert + prune). Errors if the DB
/// isn't connected yet (commands only run once the app is past the splash).
fn persist_persons(app: &tauri::AppHandle, people: &[Person]) -> Result<(), String> {
    persons::repo::save(&db_ref(app)?, people);
    Ok(())
}

// ---- commands ---------------------------------------------------------------

/// The managed person list (the hierarchy + recipient links + permissions).
#[tauri::command]
pub fn list_people(state: tauri::State<'_, PersonStore>) -> Vec<Person> {
    state.lock().unwrap().clone()
}

/// The permission actions a superior can be granted over its subtree — the single
/// source of truth for the People UI's permission toggles.
#[tauri::command]
pub fn person_permission_kinds() -> Vec<String> {
    persons::PERMISSIONS.iter().map(|s| s.to_string()).collect()
}

/// Upsert a person by `id` (the frontend assigns a UUID for new people) and
/// persist. Validates the name, the recipient label, and guards the hierarchy
/// against self/cycle parents before saving.
#[tauri::command]
pub fn save_person(
    app: tauri::AppHandle,
    state: tauri::State<'_, PersonStore>,
    person: Person,
) -> Result<(), String> {
    let mut person = person;
    person.name = person.name.trim().to_string();
    if person.id.trim().is_empty() {
        return Err("Person id is required.".into());
    }
    if person.name.is_empty() {
        return Err("Name cannot be empty.".into());
    }
    // Normalize an empty recipient/parent to None.
    person.recipient_label =
        person.recipient_label.filter(|r| !r.trim().is_empty());
    person.parent_id = person.parent_id.filter(|p| !p.trim().is_empty());
    // Drop unknown permission actions.
    person.permissions.retain(|a| persons::PERMISSIONS.contains(&a.as_str()));

    let mut people = state.lock().unwrap();
    if let Some(parent) = person.parent_id.as_deref() {
        if persons::find(&people, parent).is_none() {
            return Err("Selected parent no longer exists.".into());
        }
        if persons::would_cycle(&people, &person.id, parent) {
            return Err("That parent would create a cycle in the hierarchy.".into());
        }
    }
    if let Some(existing) = people.iter_mut().find(|p| p.id == person.id) {
        *existing = person;
    } else {
        people.push(person);
    }
    persist_persons(&app, &people)
}

/// Remove a person and detach any children that pointed to them (so the in-memory
/// store mirrors the table's `ON DELETE SET NULL`). Returns whether one was removed.
#[tauri::command]
pub fn delete_person(
    app: tauri::AppHandle,
    state: tauri::State<'_, PersonStore>,
    id: String,
) -> Result<bool, String> {
    let mut people = state.lock().unwrap();
    let before = people.len();
    people.retain(|p| p.id != id);
    let removed = people.len() != before;
    if removed {
        for child in people.iter_mut().filter(|p| p.parent_id.as_deref() == Some(id.as_str())) {
            child.parent_id = None;
        }
        persist_persons(&app, &people)?;
    }
    Ok(removed)
}

/// Enroll one face for a person: compute an embedding from `image` (base64, optionally
/// a `data:…;base64,` URL) and insert it as a new `person_embeddings` row. `source` is
/// `"camera"` or `"upload"`; `approved` carries the per-sample consent toggle. Returns
/// the new embedding's id.
#[tauri::command]
pub fn enroll_face(
    app: tauri::AppHandle,
    state: tauri::State<'_, PersonStore>,
    person_id: String,
    image: String,
    source: String,
    approved: bool,
) -> Result<String, String> {
    {
        let people = state.lock().unwrap();
        if persons::find(&people, &person_id).is_none() {
            return Err("Person not found.".into());
        }
    }
    let b64 = image.rsplit(',').next().unwrap_or(&image);
    let bytes = B64.decode(b64.trim()).map_err(|e| format!("decode image: {e}"))?;
    let embedding = app_core::agent::face::embed(&bytes)?;
    persons::repo::insert_embedding(&db_ref(&app)?, &person_id, &embedding, approved, &source)
}

/// Every enrolled embedding for a person (with the raw vector, for the avatar).
#[tauri::command]
pub fn list_embeddings(app: tauri::AppHandle, person_id: String) -> Result<Vec<PersonEmbedding>, String> {
    Ok(persons::repo::list_embeddings(&db_ref(&app)?, &person_id))
}

/// Approve a single embedding (per-sample consent).
#[tauri::command]
pub fn approve_embedding(app: tauri::AppHandle, id: String) -> Result<(), String> {
    persons::repo::approve_embedding(&db_ref(&app)?, &id)
}

/// Delete a single embedding.
#[tauri::command]
pub fn delete_embedding(app: tauri::AppHandle, id: String) -> Result<(), String> {
    persons::repo::delete_embedding(&db_ref(&app)?, &id)
}

/// Ids of people with ≥1 approved embedding — the directory's "enrolled" badge.
#[tauri::command]
pub fn enrolled_person_ids(app: tauri::AppHandle) -> Vec<String> {
    match db_ref(&app) {
        Ok(db) => persons::repo::enrolled_person_ids(&db),
        Err(_) => Vec::new(),
    }
}

/// Result of a recognition probe: the **query** embedding (so the UI can draw the
/// target's point-cloud), plus the best `match` within `max_distance` (or none).
#[derive(Serialize)]
pub struct RecognizeDetail {
    query: Vec<f32>,
    #[serde(rename = "match")]
    matched: Option<RecognitionResult>,
}

/// Recognise a face: embed `image` (base64, optionally a data URL) and match it
/// against the approved embeddings. Returns the query embedding + the best match.
#[tauri::command]
pub fn recognize_face(
    app: tauri::AppHandle,
    image: String,
    max_distance: Option<f32>,
) -> Result<RecognizeDetail, String> {
    let b64 = image.rsplit(',').next().unwrap_or(&image);
    let bytes = B64.decode(b64.trim()).map_err(|e| format!("decode image: {e}"))?;
    let query = app_core::agent::face::embed(&bytes)?;
    let matches = persons::repo::recognize(&db_ref(&app)?, &query, 5);
    let matched = persons::best_match(&matches, max_distance.unwrap_or(0.4));
    Ok(RecognizeDetail { query, matched })
}
