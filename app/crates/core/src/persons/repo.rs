//! Postgres adapter for the persons slice — owns the person + face-embedding SQL
//! (moved out of the shared `db.rs`), including the pgvector kNN `recognize`.
//! Functions take the shared [`Db`] infra handle (pool + runtime).
use crate::db::Db;

use super::{Person, PersonEmbedding, RecognitionMatch};

// Identity + hierarchy + permissions. Face biometrics live in `person_embeddings`.
const PERSON_COLS: &str = "id, name, parent_id, recipient_label, permissions";

/// The saved people, ordered by name. The `embedding` column is not selected — it's
/// read separately for recognition. Empty (and logged) on a query error.
pub fn load(db: &Db) -> Vec<Person> {
    let pool = db.pool();
    db.block(async move {
        let sql = format!("SELECT {PERSON_COLS} FROM persons ORDER BY name");
        sqlx::query_as::<_, Person>(&sql).fetch_all(&pool).await.unwrap_or_else(|e| {
            log::error!("persons::repo::load query failed: {e}");
            Vec::new()
        })
    })
}

/// Persist the full person set: upsert each row's metadata (leaving `embedding` and
/// `created_at` untouched, so an enrolled face survives a metadata edit), then delete
/// any row no longer in the in-memory set. Best-effort.
pub fn save(db: &Db, people: &[Person]) {
    let pool = db.pool();
    let people = people.to_vec();
    let res: Result<(), sqlx::Error> = db.block(async move {
        let mut tx = pool.begin().await?;
        let ids: Vec<String> = people.iter().map(|p| p.id.clone()).collect();
        sqlx::query("DELETE FROM persons WHERE NOT (id = ANY($1))")
            .bind(&ids)
            .execute(&mut *tx)
            .await?;
        for p in &people {
            sqlx::query(
                "INSERT INTO persons (id, name, parent_id, recipient_label, permissions) \
                 VALUES ($1, $2, $3, $4, $5) \
                 ON CONFLICT (id) DO UPDATE SET \
                   name = EXCLUDED.name, \
                   parent_id = EXCLUDED.parent_id, \
                   recipient_label = EXCLUDED.recipient_label, \
                   permissions = EXCLUDED.permissions",
            )
            .bind(&p.id)
            .bind(&p.name)
            .bind(&p.parent_id)
            .bind(&p.recipient_label)
            .bind(&p.permissions)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    });
    if let Err(e) = res {
        log::error!("persons::repo::save transaction failed: {e}");
    }
}

/// Insert one face embedding for `person_id` (its own uuid, individual approval) and
/// return the new row id. The vector is passed as a pgvector text literal cast to
/// `vector`, so no extra crate is needed.
pub fn insert_embedding(
    db: &Db,
    person_id: &str,
    embedding: &[f32],
    approved: bool,
    source: &str,
) -> Result<String, String> {
    let pool = db.pool();
    let person_id = person_id.to_string();
    let literal = vector_literal(embedding);
    let source = source.to_string();
    db.block(async move {
        let row: (String,) = sqlx::query_as(
            "INSERT INTO person_embeddings (person_id, embedding, approved, approved_at, source) \
             VALUES ($1, $2::vector, $3, CASE WHEN $3 THEN now() END, $4) \
             RETURNING id::text",
        )
        .bind(&person_id)
        .bind(&literal)
        .bind(approved)
        .bind(&source)
        .fetch_one(&pool)
        .await
        .map_err(|e| format!("insert_embedding: {e}"))?;
        Ok(row.0)
    })
}

/// Every embedding for `person_id`, newest first (with the raw vector for the avatar).
pub fn list_embeddings(db: &Db, person_id: &str) -> Vec<PersonEmbedding> {
    let pool = db.pool();
    let person_id = person_id.to_string();
    db.block(async move {
        sqlx::query_as::<_, PersonEmbedding>(
            "SELECT id::text AS id, person_id, approved, source, \
                    created_at::text AS created_at, embedding::float4[] AS embedding \
             FROM person_embeddings WHERE person_id = $1 ORDER BY created_at DESC",
        )
        .bind(&person_id)
        .fetch_all(&pool)
        .await
        .unwrap_or_else(|e| {
            log::error!("list_embeddings query failed: {e}");
            Vec::new()
        })
    })
}

/// Approve one embedding (sets `approved` + `approved_at`).
pub fn approve_embedding(db: &Db, id: &str) -> Result<(), String> {
    let pool = db.pool();
    let id = id.to_string();
    db.block(async move {
        sqlx::query(
            "UPDATE person_embeddings SET approved = true, approved_at = now() WHERE id = $1::uuid",
        )
        .bind(&id)
        .execute(&pool)
        .await
        .map(|_| ())
        .map_err(|e| format!("approve_embedding: {e}"))
    })
}

/// Delete one embedding by id.
pub fn delete_embedding(db: &Db, id: &str) -> Result<(), String> {
    let pool = db.pool();
    let id = id.to_string();
    db.block(async move {
        sqlx::query("DELETE FROM person_embeddings WHERE id = $1::uuid")
            .bind(&id)
            .execute(&pool)
            .await
            .map(|_| ())
            .map_err(|e| format!("delete_embedding: {e}"))
    })
}

/// Nearest people to `embedding` by cosine distance over **approved** embeddings,
/// closest first (≤ `k` rows). A person can appear more than once (one row per
/// matching sample); the caller aggregates. Empty on error.
pub fn recognize(db: &Db, embedding: &[f32], k: i64) -> Vec<RecognitionMatch> {
    let pool = db.pool();
    let literal = vector_literal(embedding);
    db.block(async move {
        let t = std::time::Instant::now();
        let rows = sqlx::query_as::<_, RecognitionMatch>(
            "SELECT pe.person_id AS person_id, p.name AS name, \
                    (pe.embedding <=> $1::vector)::float4 AS distance \
             FROM person_embeddings pe JOIN persons p ON p.id = pe.person_id \
             WHERE pe.approved ORDER BY distance LIMIT $2",
        )
        .bind(&literal)
        .bind(k)
        .fetch_all(&pool)
        .await
        .unwrap_or_else(|e| {
            log::error!("recognize query failed: {e}");
            Vec::new()
        });
        log::info!("recognize: pgvector kNN k={k} → {} rows in {:?}", rows.len(), t.elapsed());
        rows
    })
}

/// Ids of every person with ≥1 approved embedding — drives the "enrolled" badge.
pub fn enrolled_person_ids(db: &Db) -> Vec<String> {
    let pool = db.pool();
    db.block(async move {
        sqlx::query_as::<_, (String,)>("SELECT DISTINCT person_id FROM person_embeddings WHERE approved")
            .fetch_all(&pool)
            .await
            .map(|rows| rows.into_iter().map(|r| r.0).collect())
            .unwrap_or_else(|e| {
                log::error!("enrolled_person_ids query failed: {e}");
                Vec::new()
            })
    })
}

/// Format an embedding as a pgvector text literal: `[0.1,0.2,…]`.
fn vector_literal(embedding: &[f32]) -> String {
    let mut s = String::with_capacity(embedding.len() * 8 + 2);
    s.push('[');
    for (i, v) in embedding.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        s.push_str(&v.to_string());
    }
    s.push(']');
    s
}
