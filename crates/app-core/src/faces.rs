//! Face store (#117) — the tenant's captured faces: labeled (a person) or unknown, each a 512-d
//! ArcFace embedding + an optional MinIO image key. Recognition is a kNN UNION over
//! `person_embeddings` (enrolled) + `faces` (stored), so unknown faces match prior unknowns and
//! cluster before anyone labels them. Mirrors the `persons` pgvector pattern: a `[..]` string
//! literal bound as `$n::vector`, cosine distance `<=>`, and a `SET LOCAL statement_timeout` guard.

use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use crate::db::Db;
use crate::timeutil::now_rfc3339;

/// A row to store. `person_id = None` → unknown.
pub struct NewFace {
    pub person_id: Option<String>,
    pub embedding: Vec<f32>,
    pub image_key: Option<String>,
    pub camera: Option<String>,
    pub bbox: Option<serde_json::Value>,
    pub confidence: f32,
    pub face_ratio: f32,
    pub source: String,
}

/// A stored face as returned (no embedding — that stays server-side).
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Face {
    pub id: String,
    pub person_id: Option<String>,
    pub image_key: Option<String>,
    pub camera: Option<String>,
    pub confidence: f32,
    pub face_ratio: f32,
    pub source: String,
    pub created_at: String,
}

/// One kNN hit against the union gallery.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct FaceMatch {
    /// The matched person id, else the stored face's id (for an unknown).
    pub reference: String,
    /// The person's display name, or `None` for an unknown stored face.
    pub label: Option<String>,
    /// Cosine distance (0 = identical, lower is better).
    pub distance: f32,
    /// `"enrolled"` (person_embeddings) or `"stored"` (faces).
    pub source: String,
}

/// pgvector text literal `[v0,v1,…]` (mirrors `persons::repo::vector_literal`).
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

/// Insert a captured face (labeled or unknown). Returns the new face id.
pub fn insert(db: &Db, tenant_id: Uuid, f: NewFace) -> Result<String, String> {
    let pool = db.pool();
    let literal = vector_literal(&f.embedding);
    let now = now_rfc3339();
    db.block(async move {
        let row: (String,) = sqlx::query_as(
            "INSERT INTO faces (tenant_id, person_id, embedding, image_key, camera, bbox, \
                                confidence, face_ratio, source, created_at) \
             VALUES ($1, $2, $3::vector, $4, $5, $6, $7, $8, $9, $10) RETURNING id::text",
        )
        .bind(tenant_id)
        .bind(&f.person_id)
        .bind(&literal)
        .bind(&f.image_key)
        .bind(&f.camera)
        .bind(&f.bbox)
        .bind(f.confidence)
        .bind(f.face_ratio)
        .bind(&f.source)
        .bind(&now)
        .fetch_one(&pool)
        .await
        .map_err(|e| format!("faces::insert: {e}"))?;
        Ok(row.0)
    })
}

/// kNN over the UNION of enrolled (`person_embeddings`, approved) + stored (`faces`) galleries,
/// closest first (≤ `k` rows). Each hit is tagged `source`. Empty on error.
pub fn recognize(db: &Db, tenant_id: Uuid, embedding: &[f32], k: i64) -> Vec<FaceMatch> {
    let pool = db.pool();
    let literal = vector_literal(embedding);
    db.block(async move {
        let result: Result<Vec<FaceMatch>, sqlx::Error> = async {
            let mut tx = pool.begin().await?;
            sqlx::query("SET LOCAL statement_timeout = 15000").execute(&mut *tx).await?;
            let rows = sqlx::query_as::<_, FaceMatch>(
                "SELECT reference, label, distance, source FROM ( \
                   SELECT pe.person_id AS reference, p.name AS label, \
                          (pe.embedding <=> $1::vector)::float4 AS distance, 'enrolled' AS source \
                     FROM person_embeddings pe \
                     JOIN persons p ON p.id = pe.person_id AND p.tenant_id = pe.tenant_id \
                     WHERE pe.approved AND pe.tenant_id = $2 \
                   UNION ALL \
                   SELECT COALESCE(f.person_id, f.id::text) AS reference, p2.name AS label, \
                          (f.embedding <=> $1::vector)::float4 AS distance, 'stored' AS source \
                     FROM faces f \
                     LEFT JOIN persons p2 ON p2.id = f.person_id AND p2.tenant_id = f.tenant_id \
                     WHERE f.tenant_id = $2 \
                 ) u ORDER BY distance LIMIT $3",
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
        result.unwrap_or_else(|e| {
            log::error!("faces::recognize query failed: {e}");
            Vec::new()
        })
    })
}

/// Nearest cosine distance to any already-stored face (for `StoreFace` dedup). `None` on empty/error.
pub fn nearest_stored_distance(db: &Db, tenant_id: Uuid, embedding: &[f32]) -> Option<f32> {
    let pool = db.pool();
    let literal = vector_literal(embedding);
    db.block(async move {
        sqlx::query_as::<_, (f32,)>(
            "SELECT (embedding <=> $1::vector)::float4 AS distance \
             FROM faces WHERE tenant_id = $2 ORDER BY distance LIMIT 1",
        )
        .bind(&literal)
        .bind(tenant_id)
        .fetch_optional(&pool)
        .await
        .ok()
        .flatten()
        .map(|(d,)| d)
    })
}

/// The tenant's stored faces, newest first (for the later review/cluster views).
pub fn list(db: &Db, tenant_id: Uuid, limit: i64) -> Vec<Face> {
    let pool = db.pool();
    db.block(async move {
        sqlx::query_as::<_, Face>(
            "SELECT id::text AS id, person_id, image_key, camera, confidence, face_ratio, source, \
                    created_at \
             FROM faces WHERE tenant_id = $1 AND NOT ignored ORDER BY created_at DESC LIMIT $2",
        )
        .bind(tenant_id)
        .bind(limit)
        .fetch_all(&pool)
        .await
        .unwrap_or_else(|e| {
            log::error!("faces::list query failed: {e}");
            Vec::new()
        })
    })
}

// ---- Faces labeling view (#119) --------------------------------------------------------------

/// Suggested identity for a cluster: the enrolled person its centroid recognizes as.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterSuggestion {
    pub person_id: String,
    pub name: String,
    pub confidence: i64, // 0..100
}

/// A cluster of similar unknown faces — the unit of labeling in the triage view.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FaceCluster {
    /// Stable key for UI keying (the representative face id — unique per cluster).
    pub key: String,
    pub face_ids: Vec<String>,
    pub representative: Face,
    pub size: usize,
    pub suggestion: Option<ClusterSuggestion>,
}

/// One unknown face row + its embedding (clustering input).
#[derive(FromRow)]
struct FaceEmbRow {
    id: String,
    person_id: Option<String>,
    image_key: Option<String>,
    camera: Option<String>,
    confidence: f32,
    face_ratio: f32,
    source: String,
    created_at: String,
    embedding: Vec<f32>,
}

impl FaceEmbRow {
    fn into_face(self) -> (Face, Vec<f32>) {
        let emb = self.embedding;
        (
            Face {
                id: self.id,
                person_id: self.person_id,
                image_key: self.image_key,
                camera: self.camera,
                confidence: self.confidence,
                face_ratio: self.face_ratio,
                source: self.source,
                created_at: self.created_at,
            },
            emb,
        )
    }
}

/// Cosine distance threshold for one-cluster-per-identity — equal to the recognition `max_distance`
/// default, so faces that already recognize each other land in the same component.
const CLUSTER_EPS: f32 = 0.40;
/// Cap the O(n²) clustering pass — beyond this the unknown pile is degenerate; a flat list is served.
const MAX_CLUSTER_N: i64 = 2000;

/// Cosine distance of two L2-normalized ArcFace vectors (`1 - dot`).
fn cosine_distance(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    1.0 - dot
}

/// Group the tenant's UNKNOWN, non-ignored faces into clusters of similar faces (single-link
/// connected components at [`CLUSTER_EPS`]), each with an auto-suggested enrolled person (from the
/// cluster centroid). Largest clusters first. On-demand; empty on error.
pub fn cluster_unlabeled(db: &Db, tenant_id: Uuid) -> Vec<FaceCluster> {
    let pool = db.pool();
    let items: Vec<(Face, Vec<f32>)> = db.block(async move {
        sqlx::query_as::<_, FaceEmbRow>(
            "SELECT id::text AS id, person_id, image_key, camera, confidence, face_ratio, source, \
                    created_at, embedding::float4[] AS embedding \
             FROM faces WHERE tenant_id = $1 AND person_id IS NULL AND NOT ignored \
             ORDER BY created_at DESC LIMIT $2",
        )
        .bind(tenant_id)
        .bind(MAX_CLUSTER_N)
        .fetch_all(&pool)
        .await
        .unwrap_or_else(|e| {
            log::error!("faces::cluster_unlabeled query failed: {e}");
            Vec::new()
        })
    })
    .into_iter()
    .map(FaceEmbRow::into_face)
    .collect();

    let n = items.len();
    if n == 0 {
        return Vec::new();
    }

    // Union-find over pairs within CLUSTER_EPS.
    let mut parent: Vec<usize> = (0..n).collect();
    fn find(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]]; // path halving
            i = parent[i];
        }
        i
    }
    for i in 0..n {
        for j in (i + 1)..n {
            if cosine_distance(&items[i].1, &items[j].1) <= CLUSTER_EPS {
                let (ri, rj) = (find(&mut parent, i), find(&mut parent, j));
                if ri != rj {
                    parent[ri] = rj;
                }
            }
        }
    }

    // Group indices by root.
    let mut groups: std::collections::HashMap<usize, Vec<usize>> = std::collections::HashMap::new();
    for i in 0..n {
        let r = find(&mut parent, i);
        groups.entry(r).or_default().push(i);
    }

    let dim = items[0].1.len();
    let mut clusters: Vec<FaceCluster> = groups
        .into_values()
        .map(|idxs| {
            // Representative = highest-confidence face.
            let rep = *idxs
                .iter()
                .max_by(|&&a, &&b| items[a].0.confidence.total_cmp(&items[b].0.confidence))
                .unwrap();
            let representative = items[rep].0.clone();
            let face_ids: Vec<String> = idxs.iter().map(|&i| items[i].0.id.clone()).collect();

            // Centroid (mean, renormalized) → auto-suggest against enrolled people.
            let mut centroid = vec![0.0f32; dim];
            for &i in &idxs {
                for (k, v) in items[i].1.iter().enumerate() {
                    centroid[k] += v;
                }
            }
            let norm = centroid.iter().map(|x| x * x).sum::<f32>().sqrt();
            if norm > 0.0 {
                for x in &mut centroid {
                    *x /= norm;
                }
            }
            let suggestion = crate::persons::best_match(
                &crate::persons::repo::recognize(db, tenant_id, &centroid, 5),
                CLUSTER_EPS,
            )
            .map(|m| ClusterSuggestion { person_id: m.person_id, name: m.name, confidence: m.confidence });

            FaceCluster { key: representative.id.clone(), size: face_ids.len(), face_ids, representative, suggestion }
        })
        .collect();

    clusters.sort_by(|a, b| b.size.cmp(&a.size));
    clusters
}

/// One face by id (tenant-scoped).
pub fn get(db: &Db, tenant_id: Uuid, id: &str) -> Option<Face> {
    let pool = db.pool();
    let id = id.to_string();
    db.block(async move {
        sqlx::query_as::<_, Face>(
            "SELECT id::text AS id, person_id, image_key, camera, confidence, face_ratio, source, \
                    created_at FROM faces WHERE tenant_id = $1 AND id = $2::uuid",
        )
        .bind(tenant_id)
        .bind(&id)
        .fetch_optional(&pool)
        .await
        .ok()
        .flatten()
    })
}

/// Cheap image-key lookup for the image proxy. Outer `None` = no such face in this tenant; inner
/// `None` = the face has no stored image.
pub fn image_key_of(db: &Db, tenant_id: Uuid, id: &str) -> Option<Option<String>> {
    let pool = db.pool();
    let id = id.to_string();
    db.block(async move {
        sqlx::query_as::<_, (Option<String>,)>(
            "SELECT image_key FROM faces WHERE tenant_id = $1 AND id = $2::uuid",
        )
        .bind(tenant_id)
        .bind(&id)
        .fetch_optional(&pool)
        .await
        .ok()
        .flatten()
        .map(|(k,)| k)
    })
}

/// Set (or clear, with `None`) the label on faces — the reassign path. Returns rows affected.
pub fn set_person(db: &Db, tenant_id: Uuid, ids: &[String], person_id: Option<&str>) -> Result<u64, String> {
    let pool = db.pool();
    let ids = ids.to_vec();
    let person_id = person_id.map(str::to_string);
    db.block(async move {
        sqlx::query("UPDATE faces SET person_id = $2 WHERE tenant_id = $1 AND id = ANY($3::uuid[])")
            .bind(tenant_id)
            .bind(&person_id)
            .bind(&ids)
            .execute(&pool)
            .await
            .map(|r| r.rows_affected())
            .map_err(|e| format!("faces::set_person: {e}"))
    })
}

/// Soft dismiss/restore junk crops. Returns rows affected.
pub fn set_ignored(db: &Db, tenant_id: Uuid, ids: &[String], ignored: bool) -> Result<u64, String> {
    let pool = db.pool();
    let ids = ids.to_vec();
    db.block(async move {
        sqlx::query("UPDATE faces SET ignored = $2 WHERE tenant_id = $1 AND id = ANY($3::uuid[])")
            .bind(tenant_id)
            .bind(ignored)
            .bind(&ids)
            .execute(&pool)
            .await
            .map(|r| r.rows_affected())
            .map_err(|e| format!("faces::set_ignored: {e}"))
    })
}

/// **The promote transaction (#119).** Label a set of faces as `person_id` (which must already
/// exist): copy each face's embedding into `person_embeddings` (approved, so it recognizes next time)
/// AND stamp `faces.person_id` — all-or-nothing in one tx. The `INSERT … SELECT` copies vectors
/// server-side (no round-trip). Returns the number of faces labeled.
pub fn label_faces(db: &Db, tenant_id: Uuid, ids: &[String], person_id: &str) -> Result<u64, String> {
    let pool = db.pool();
    let ids = ids.to_vec();
    let person_id = person_id.to_string();
    db.block(async move {
        let mut tx = pool.begin().await.map_err(|e| e.to_string())?;
        sqlx::query(
            "INSERT INTO person_embeddings (tenant_id, person_id, embedding, approved, approved_at, source) \
             SELECT f.tenant_id, $2, f.embedding, true, now(), 'labeled' \
               FROM faces f WHERE f.tenant_id = $1 AND f.id = ANY($3::uuid[])",
        )
        .bind(tenant_id)
        .bind(&person_id)
        .bind(&ids)
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("faces::label_faces enroll: {e}"))?;
        let res = sqlx::query("UPDATE faces SET person_id = $2 WHERE tenant_id = $1 AND id = ANY($3::uuid[])")
            .bind(tenant_id)
            .bind(&person_id)
            .bind(&ids)
            .execute(&mut *tx)
            .await
            .map_err(|e| format!("faces::label_faces stamp: {e}"))?;
        tx.commit().await.map_err(|e| e.to_string())?;
        Ok(res.rows_affected())
    })
}

/// Delete faces (hard), returning their `image_key`s so the caller can purge the MinIO objects.
pub fn delete(db: &Db, tenant_id: Uuid, ids: &[String]) -> Result<Vec<String>, String> {
    let pool = db.pool();
    let ids = ids.to_vec();
    db.block(async move {
        let keys: Vec<(Option<String>,)> = sqlx::query_as(
            "DELETE FROM faces WHERE tenant_id = $1 AND id = ANY($2::uuid[]) RETURNING image_key",
        )
        .bind(tenant_id)
        .bind(&ids)
        .fetch_all(&pool)
        .await
        .map_err(|e| format!("faces::delete: {e}"))?;
        Ok(keys.into_iter().filter_map(|(k,)| k).collect())
    })
}
