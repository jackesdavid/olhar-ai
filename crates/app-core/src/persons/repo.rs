//! Postgres adapter for the persons slice — owns the person + face-embedding SQL
//! (moved out of the shared `db.rs`), including the pgvector kNN `recognize`. Every
//! function is **tenant-scoped**: rows are keyed `(tenant_id, …)` and never crossed.
use uuid::Uuid;

use crate::db::Db;

use super::{Person, PersonEmbedding, RecognitionMatch};

// Identity + hierarchy + permissions. Face biometrics live in `person_embeddings`.
const PERSON_COLS: &str = "id, name, parent_id, recipient_label, permissions";

/// The tenant's people, ordered by name. Empty (and logged) on a query error.
pub fn load(db: &Db, tenant_id: Uuid) -> Vec<Person> {
    let pool = db.pool();
    db.block(async move {
        let sql = format!("SELECT {PERSON_COLS} FROM persons WHERE tenant_id = $1 ORDER BY name");
        sqlx::query_as::<_, Person>(&sql)
            .bind(tenant_id)
            .fetch_all(&pool)
            .await
            .unwrap_or_else(|e| {
                log::error!("persons::repo::load query failed: {e}");
                Vec::new()
            })
    })
}

/// Persist the tenant's person set: upsert each row's metadata (leaving `embedding`
/// and `created_at` untouched), then delete any of the tenant's rows no longer in the
/// in-memory set. Best-effort.
pub fn save(db: &Db, tenant_id: Uuid, people: &[Person]) {
    let pool = db.pool();
    let people = people.to_vec();
    let res: Result<(), sqlx::Error> = db.block(async move {
        let mut tx = pool.begin().await?;
        let ids: Vec<String> = people.iter().map(|p| p.id.clone()).collect();
        sqlx::query("DELETE FROM persons WHERE tenant_id = $1 AND NOT (id = ANY($2))")
            .bind(tenant_id)
            .bind(&ids)
            .execute(&mut *tx)
            .await?;
        for p in &people {
            sqlx::query(
                "INSERT INTO persons (tenant_id, id, name, parent_id, recipient_label, permissions) \
                 VALUES ($1, $2, $3, $4, $5, $6) \
                 ON CONFLICT (tenant_id, id) DO UPDATE SET \
                   name = EXCLUDED.name, \
                   parent_id = EXCLUDED.parent_id, \
                   recipient_label = EXCLUDED.recipient_label, \
                   permissions = EXCLUDED.permissions",
            )
            .bind(tenant_id)
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

/// Insert one face embedding for `person_id` and return the new row id.
pub fn insert_embedding(
    db: &Db,
    tenant_id: Uuid,
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
            "INSERT INTO person_embeddings (tenant_id, person_id, embedding, approved, approved_at, source) \
             VALUES ($1, $2, $3::vector, $4, CASE WHEN $4 THEN now() END, $5) \
             RETURNING id::text",
        )
        .bind(tenant_id)
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

/// Every embedding for `person_id` (within the tenant), newest first.
pub fn list_embeddings(db: &Db, tenant_id: Uuid, person_id: &str) -> Vec<PersonEmbedding> {
    let pool = db.pool();
    let person_id = person_id.to_string();
    db.block(async move {
        sqlx::query_as::<_, PersonEmbedding>(
            "SELECT id::text AS id, person_id, approved, source, \
                    created_at::text AS created_at, embedding::float4[] AS embedding \
             FROM person_embeddings WHERE tenant_id = $1 AND person_id = $2 ORDER BY created_at DESC",
        )
        .bind(tenant_id)
        .bind(&person_id)
        .fetch_all(&pool)
        .await
        .unwrap_or_else(|e| {
            log::error!("list_embeddings query failed: {e}");
            Vec::new()
        })
    })
}

/// Approve one embedding (within the tenant).
pub fn approve_embedding(db: &Db, tenant_id: Uuid, id: &str) -> Result<(), String> {
    let pool = db.pool();
    let id = id.to_string();
    db.block(async move {
        sqlx::query(
            "UPDATE person_embeddings SET approved = true, approved_at = now() \
             WHERE tenant_id = $1 AND id = $2::uuid",
        )
        .bind(tenant_id)
        .bind(&id)
        .execute(&pool)
        .await
        .map(|_| ())
        .map_err(|e| format!("approve_embedding: {e}"))
    })
}

/// Delete one embedding by id (within the tenant).
pub fn delete_embedding(db: &Db, tenant_id: Uuid, id: &str) -> Result<(), String> {
    let pool = db.pool();
    let id = id.to_string();
    db.block(async move {
        sqlx::query("DELETE FROM person_embeddings WHERE tenant_id = $1 AND id = $2::uuid")
            .bind(tenant_id)
            .bind(&id)
            .execute(&pool)
            .await
            .map(|_| ())
            .map_err(|e| format!("delete_embedding: {e}"))
    })
}

/// Nearest people to `embedding` by cosine distance over the tenant's **approved**
/// embeddings, closest first (≤ `k` rows). Empty on error.
pub fn recognize(db: &Db, tenant_id: Uuid, embedding: &[f32], k: i64) -> Vec<RecognitionMatch> {
    let pool = db.pool();
    let literal = vector_literal(embedding);
    db.block(async move {
        let t = std::time::Instant::now();
        // Run the kNN inside a short tx that first bounds it with a statement timeout, so a
        // slow/locked query aborts and releases the connection instead of hanging forever
        // (scoped per-query via SET LOCAL — never a global pool setting that could break
        // migrations or long admin queries).
        let result: Result<Vec<RecognitionMatch>, sqlx::Error> = async {
            let mut tx = pool.begin().await?;
            sqlx::query("SET LOCAL statement_timeout = 15000")
                .execute(&mut *tx)
                .await?;
            let rows = sqlx::query_as::<_, RecognitionMatch>(
                "SELECT pe.person_id AS person_id, p.name AS name, \
                        (pe.embedding <=> $1::vector)::float4 AS distance \
                 FROM person_embeddings pe \
                 JOIN persons p ON p.id = pe.person_id AND p.tenant_id = pe.tenant_id \
                 WHERE pe.approved AND pe.tenant_id = $2 ORDER BY distance LIMIT $3",
            )
            .bind(&literal)
            .bind(tenant_id)
            .bind(k)
            .fetch_all(&mut *tx)
            .await?;
            tx.commit().await?;
            Ok(rows)
        }
        .await;
        match result {
            Ok(rows) => {
                log::info!(
                    "recognize: pgvector kNN k={k} → {} rows in {:?}",
                    rows.len(),
                    t.elapsed()
                );
                rows
            }
            Err(e) => {
                log::error!("recognize query failed: {e}");
                Vec::new()
            }
        }
    })
}

/// Ids of the tenant's people with ≥1 approved embedding — drives the "enrolled" badge.
pub fn enrolled_person_ids(db: &Db, tenant_id: Uuid) -> Vec<String> {
    let pool = db.pool();
    db.block(async move {
        sqlx::query_as::<_, (String,)>(
            "SELECT DISTINCT person_id FROM person_embeddings WHERE approved AND tenant_id = $1",
        )
        .bind(tenant_id)
        .fetch_all(&pool)
        .await
        .map(|rows| rows.into_iter().map(|r| r.0).collect())
        .unwrap_or_else(|e| {
            log::error!("enrolled_person_ids query failed: {e}");
            Vec::new()
        })
    })
}

/// L2-normalize an embedding to unit length (card #125). ArcFace vectors *should* already be
/// unit-norm, but guaranteeing it at store time keeps the DB `<=>` cosine path and the in-Rust
/// `1 - dot` clustering path in agreement, so sidecar contract drift can't silently skew results.
/// A zero vector is passed through unchanged. (Mirrors `faces::l2_normalize`.)
fn l2_normalize(embedding: &[f32]) -> Vec<f32> {
    let norm = embedding.iter().map(|x| x * x).sum::<f32>().sqrt();
    debug_assert!(
        norm == 0.0 || (norm - 1.0).abs() < 1e-2,
        "person embedding not unit-norm at store time (‖v‖={norm}); sidecar contract drift?"
    );
    if norm > 0.0 {
        embedding.iter().map(|x| x / norm).collect()
    } else {
        embedding.to_vec()
    }
}

/// Format an embedding as a pgvector text literal: `[0.1,0.2,…]`, L2-normalized (card #125).
fn vector_literal(embedding: &[f32]) -> String {
    let embedding = l2_normalize(embedding);
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
