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
    /// Pre-cropped face thumbnail key (the proxy serves it as a passthrough); `None` = crop on read.
    pub crop_key: Option<String>,
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
                                confidence, face_ratio, source, created_at, crop_key) \
             VALUES ($1, $2, $3::vector, $4, $5, $6, $7, $8, $9, $10, $11) RETURNING id::text",
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
        .bind(&f.crop_key)
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

/// Cosine distance threshold for grouping faces into one identity. Deliberately LOOSER than the
/// recognition `max_distance` (0.40): recognition is a strict 1:1 gate, but clustering groups
/// SURVEILLANCE crops of the same person — small, varied pose/lighting — whose ArcFace vectors spread
/// wider (measured intra-person nearest-neighbour ~0.50–0.56 on real captures). At 0.40 nothing merges
/// and every face is its own cluster; 0.55 groups same-person captures while staying below the
/// cross-identity range. (Tunable later per-tenant if faces get higher-res.)
pub const CLUSTER_EPS: f32 = 0.55;
/// Cap the O(n²) clustering pass — beyond this the unknown pile is degenerate; a flat list is served.
const MAX_CLUSTER_N: i64 = 2000;

/// Cosine distance of two L2-normalized ArcFace vectors (`1 - dot`).
fn cosine_distance(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    1.0 - dot
}

/// Internal per-cluster core shared by the list (`cluster_unlabeled`) and canvas (`layout_unlabeled`)
/// views: the member indices, its highest-confidence representative, the renormalized centroid, and
/// the enrolled-person suggestion.
struct ClusterCore {
    idxs: Vec<usize>,
    representative: Face,
    face_ids: Vec<String>,
    centroid: Vec<f32>,
    suggestion: Option<ClusterSuggestion>,
    /// Person name when this is a KNOWN (labeled) island; `None` for an unknown cluster.
    label: Option<String>,
}

/// Which faces a canvas/list view shows.
#[derive(Clone, Copy, PartialEq)]
pub enum FaceScope {
    /// Unlabeled captures awaiting a name (the default triage set).
    Unknown,
    /// Already-labeled faces, grouped by person (the "anchor" islands).
    Known,
    /// Both — labeled anchors + the unknown pile.
    All,
}

impl FaceScope {
    pub fn parse(s: &str) -> FaceScope {
        match s {
            "known" => FaceScope::Known,
            "all" => FaceScope::All,
            _ => FaceScope::Unknown,
        }
    }
    /// SQL predicate (all cases exclude ignored faces).
    fn predicate(&self) -> &'static str {
        match self {
            FaceScope::Unknown => "person_id IS NULL AND NOT ignored",
            FaceScope::Known => "person_id IS NOT NULL AND NOT ignored",
            FaceScope::All => "NOT ignored",
        }
    }
}

/// Load faces (+ embeddings) matching `scope`, newest first, capped at `MAX_CLUSTER_N`.
fn load_faces(db: &Db, tenant_id: Uuid, scope: FaceScope) -> Vec<(Face, Vec<f32>)> {
    let pool = db.pool();
    let sql = format!(
        "SELECT id::text AS id, person_id, image_key, camera, confidence, face_ratio, source, \
                created_at, embedding::float4[] AS embedding \
         FROM faces WHERE tenant_id = $1 AND {} \
         ORDER BY created_at DESC LIMIT $2",
        scope.predicate()
    );
    db.block(async move {
        sqlx::query_as::<_, FaceEmbRow>(&sql)
            .bind(tenant_id)
            .bind(MAX_CLUSTER_N)
            .fetch_all(&pool)
            .await
            .unwrap_or_else(|e| {
                log::error!("faces::load_faces query failed: {e}");
                Vec::new()
            })
    })
    .into_iter()
    .map(FaceEmbRow::into_face)
    .collect()
}

/// Single-link connected components (union-find) over faces within cosine distance `eps`.
fn build_groups(items: &[(Face, Vec<f32>)], eps: f32) -> Vec<Vec<usize>> {
    let n = items.len();
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
            if cosine_distance(&items[i].1, &items[j].1) <= eps {
                let (ri, rj) = (find(&mut parent, i), find(&mut parent, j));
                if ri != rj {
                    parent[ri] = rj;
                }
            }
        }
    }
    let mut groups: std::collections::HashMap<usize, Vec<usize>> = std::collections::HashMap::new();
    for i in 0..n {
        let r = find(&mut parent, i);
        groups.entry(r).or_default().push(i);
    }
    groups.into_values().collect()
}

/// Turn index-groups into cluster cores (representative, centroid, suggestion), largest first.
fn build_cores(db: &Db, tenant_id: Uuid, items: &[(Face, Vec<f32>)], eps: f32) -> Vec<ClusterCore> {
    if items.is_empty() {
        return Vec::new();
    }
    let dim = items[0].1.len();
    let mut cores: Vec<ClusterCore> = build_groups(items, eps)
        .into_iter()
        .map(|idxs| {
            let rep = *idxs
                .iter()
                .max_by(|&&a, &&b| items[a].0.confidence.total_cmp(&items[b].0.confidence))
                .unwrap();
            let representative = items[rep].0.clone();
            let face_ids: Vec<String> = idxs.iter().map(|&i| items[i].0.id.clone()).collect();

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
                eps,
            )
            .map(|m| ClusterSuggestion { person_id: m.person_id, name: m.name, confidence: m.confidence });

            ClusterCore { idxs, representative, face_ids, centroid, suggestion, label: None }
        })
        .collect();
    cores.sort_by(|a, b| b.face_ids.len().cmp(&a.face_ids.len()));
    cores
}

/// Group KNOWN (labeled) faces into one core per person — the "anchor" islands. No embedding
/// clustering; the person id IS the group. `name_of` maps person id → display name.
fn build_person_cores(
    items: &[(Face, Vec<f32>)],
    name_of: &std::collections::HashMap<String, String>,
) -> Vec<ClusterCore> {
    if items.is_empty() {
        return Vec::new();
    }
    let dim = items[0].1.len();
    let mut by_person: std::collections::HashMap<String, Vec<usize>> = std::collections::HashMap::new();
    for (i, (f, _)) in items.iter().enumerate() {
        if let Some(pid) = &f.person_id {
            by_person.entry(pid.clone()).or_default().push(i);
        }
    }
    let mut cores: Vec<ClusterCore> = by_person
        .into_iter()
        .map(|(pid, idxs)| {
            let rep = *idxs
                .iter()
                .max_by(|&&a, &&b| items[a].0.confidence.total_cmp(&items[b].0.confidence))
                .unwrap();
            let representative = items[rep].0.clone();
            let face_ids: Vec<String> = idxs.iter().map(|&i| items[i].0.id.clone()).collect();
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
            let label = Some(name_of.get(&pid).cloned().unwrap_or(pid));
            ClusterCore { idxs, representative, face_ids, centroid, suggestion: None, label }
        })
        .collect();
    cores.sort_by(|a, b| b.face_ids.len().cmp(&a.face_ids.len()));
    cores
}

/// Group the tenant's UNKNOWN, non-ignored faces into clusters of similar faces (single-link
/// connected components at `eps`), each with an auto-suggested enrolled person (from the cluster
/// centroid). Largest clusters first. On-demand; empty on error.
pub fn cluster_unlabeled(db: &Db, tenant_id: Uuid, eps: f32) -> Vec<FaceCluster> {
    let eps = eps.clamp(0.20, 0.90);
    let items = load_faces(db, tenant_id, FaceScope::Unknown);
    build_cores(db, tenant_id, &items, eps)
        .into_iter()
        .map(|c| FaceCluster {
            key: c.representative.id.clone(),
            size: c.face_ids.len(),
            face_ids: c.face_ids,
            representative: c.representative,
            suggestion: c.suggestion,
        })
        .collect()
}

// ---- 2D canvas layout (#119 Phase 1): islands positioned by similarity ----

/// One face placed on the canvas (absolute pixel coords).
#[derive(Serialize)]
pub struct FaceDot {
    pub id: String,
    pub x: f32,
    pub y: f32,
}

/// A cluster rendered as a spatial "island": a centre, a radius sized by member count, the fanned
/// face positions, and the same representative/suggestion as the list view.
#[derive(Serialize)]
pub struct Island {
    pub key: String,
    pub size: usize,
    pub cx: f32,
    pub cy: f32,
    pub radius: f32,
    pub representative: Face,
    pub suggestion: Option<ClusterSuggestion>,
    /// Person name when this is a labeled "anchor" island; `None` for an unknown cluster.
    pub label: Option<String>,
    pub faces: Vec<FaceDot>,
}

/// The whole canvas: islands positioned so similar clusters sit near each other.
#[derive(Serialize)]
pub struct CanvasLayout {
    pub islands: Vec<Island>,
}

/// Lay faces out as similarity islands, filtered by `scope`. Unknown faces are embedding-clustered
/// at `eps`; KNOWN faces are grouped into one "anchor" island per person. Island centres are placed
/// by stress majorization (inter-island distance ∝ centroid cosine distance) and each island's faces
/// fanned in a phyllotaxis spiral. Absolute pixel coordinates; largest island first.
pub fn layout_faces(db: &Db, tenant_id: Uuid, eps: f32, scope: FaceScope) -> CanvasLayout {
    let eps = eps.clamp(0.20, 0.90);
    let items = load_faces(db, tenant_id, scope);
    // Split labeled vs unlabeled — each groups differently (by person vs by embedding).
    let (known, unknown): (Vec<_>, Vec<_>) = items.into_iter().partition(|(f, _)| f.person_id.is_some());
    let name_of: std::collections::HashMap<String, String> = if known.is_empty() {
        std::collections::HashMap::new()
    } else {
        crate::persons::repo::load(db, tenant_id).into_iter().map(|p| (p.id, p.name)).collect()
    };
    let mut cores = build_person_cores(&known, &name_of);
    cores.extend(build_cores(db, tenant_id, &unknown, eps));
    cores.sort_by(|a, b| b.face_ids.len().cmp(&a.face_ids.len()));
    if cores.is_empty() {
        return CanvasLayout { islands: Vec::new() };
    }

    // Island radius grows with sqrt(count) so face density stays roughly constant.
    let radius_of = |n: usize| 44.0 + 15.0 * (n as f32).sqrt();

    // Pairwise centroid cosine distances → 2D centre positions (stress majorization).
    let k = cores.len();
    let mut dist = vec![vec![0.0f32; k]; k];
    for i in 0..k {
        for j in (i + 1)..k {
            let d = cosine_distance(&cores[i].centroid, &cores[j].centroid);
            dist[i][j] = d;
            dist[j][i] = d;
        }
    }
    let centres = stress_layout(&dist, &cores.iter().map(|c| radius_of(c.face_ids.len())).collect::<Vec<_>>());

    let islands = cores
        .into_iter()
        .enumerate()
        .map(|(ci, c)| {
            let (cx, cy) = centres[ci];
            let r = radius_of(c.face_ids.len());
            // Phyllotaxis (sunflower) fan: even, deterministic packing around the centre.
            let n = c.idxs.len();
            let faces: Vec<FaceDot> = c
                .idxs
                .iter()
                .enumerate()
                .map(|(fi, &_idx)| {
                    let fr = if n <= 1 { 0.0 } else { r * 0.82 * ((fi as f32 + 0.5) / n as f32).sqrt() };
                    let theta = fi as f32 * 2.399_963_2; // golden angle
                    FaceDot {
                        id: c.face_ids[fi].clone(),
                        x: cx + fr * theta.cos(),
                        y: cy + fr * theta.sin(),
                    }
                })
                .collect();
            Island {
                key: c.label.clone().unwrap_or_else(|| c.representative.id.clone()),
                size: c.face_ids.len(),
                cx,
                cy,
                radius: r,
                representative: c.representative,
                suggestion: c.suggestion,
                label: c.label,
                faces,
            }
        })
        .collect();

    CanvasLayout { islands }
}

/// Place `k` points in 2D so pairwise screen distance tracks `dist` (cosine), keeping islands at
/// least their combined `radii` apart. Deterministic golden-angle init + gradient descent on the
/// stress `Σ(target − d)²` — no RNG (stable layouts), no external lib.
fn stress_layout(dist: &[Vec<f32>], radii: &[f32]) -> Vec<(f32, f32)> {
    let k = dist.len();
    if k == 0 {
        return Vec::new();
    }
    if k == 1 {
        return vec![(0.0, 0.0)];
    }
    // Pixels per unit cosine distance — sets how far apart similarity pushes islands.
    const SCALE: f32 = 520.0;
    let mut pos: Vec<(f32, f32)> = (0..k)
        .map(|i| {
            let a = i as f32 * 2.399_963_2;
            let r = 30.0 + i as f32 * 8.0;
            (r * a.cos(), r * a.sin())
        })
        .collect();
    for _ in 0..300 {
        let mut disp = vec![(0.0f32, 0.0f32); k];
        for i in 0..k {
            for j in 0..k {
                if i == j {
                    continue;
                }
                let dx = pos[i].0 - pos[j].0;
                let dy = pos[i].1 - pos[j].1;
                let d = (dx * dx + dy * dy).sqrt().max(1.0);
                // Target = similarity distance, but never closer than the two islands' radii (+gap).
                let target = (dist[i][j] * SCALE).max(radii[i] + radii[j] + 24.0);
                let f = (target - d) / d * 0.5; // >0 apart, <0 together
                disp[i].0 += dx * f;
                disp[i].1 += dy * f;
            }
        }
        for i in 0..k {
            pos[i].0 += disp[i].0 / k as f32;
            pos[i].1 += disp[i].1 / k as f32;
        }
    }
    pos
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

/// Everything the face-detail view shows: the stored row + its bbox + the person's display name.
#[derive(Serialize)]
pub struct FaceDetail {
    #[serde(flatten)]
    pub face: Face,
    pub bbox: Option<[i64; 4]>,
    pub person_name: Option<String>,
}

#[derive(sqlx::FromRow)]
struct FaceDetailRow {
    id: String,
    person_id: Option<String>,
    image_key: Option<String>,
    camera: Option<String>,
    confidence: f32,
    face_ratio: f32,
    source: String,
    created_at: String,
    bbox: Option<serde_json::Value>,
    person_name: Option<String>,
}

/// Full detail for one face (row + bbox + person name). `None` if not found / wrong tenant.
pub fn get_detail(db: &Db, tenant_id: Uuid, id: &str) -> Option<FaceDetail> {
    let pool = db.pool();
    let id = id.to_string();
    db.block(async move {
        let row: Option<FaceDetailRow> = sqlx::query_as(
            "SELECT f.id::text AS id, f.person_id, f.image_key, f.camera, f.confidence, \
                    f.face_ratio, f.source, f.created_at, f.bbox, p.name AS person_name \
             FROM faces f \
             LEFT JOIN persons p ON p.tenant_id = f.tenant_id AND p.id = f.person_id \
             WHERE f.tenant_id = $1 AND f.id = $2::uuid",
        )
        .bind(tenant_id)
        .bind(&id)
        .fetch_optional(&pool)
        .await
        .ok()
        .flatten();
        row.map(|r| {
            let bb = r.bbox.and_then(|v| {
                let a = v.as_array()?;
                if a.len() != 4 {
                    return None;
                }
                Some([a[0].as_i64()?, a[1].as_i64()?, a[2].as_i64()?, a[3].as_i64()?])
            });
            FaceDetail {
                face: Face {
                    id: r.id,
                    person_id: r.person_id,
                    image_key: r.image_key,
                    camera: r.camera,
                    confidence: r.confidence,
                    face_ratio: r.face_ratio,
                    source: r.source,
                    created_at: r.created_at,
                },
                bbox: bb,
                person_name: r.person_name,
            }
        })
    })
}

/// Fetch `(image_key, crop_key, bbox)` for a face — for the image proxy. Outer `None` = the face
/// doesn't exist (or wrong tenant). `crop_key` (when present) is a pre-cropped thumbnail the proxy
/// can serve as a passthrough; `image_key` is the full frame (for Context / on-the-fly crop).
pub fn crop_info(
    db: &Db,
    tenant_id: Uuid,
    id: &str,
) -> Option<(Option<String>, Option<String>, Option<[i64; 4]>)> {
    let pool = db.pool();
    let id = id.to_string();
    db.block(async move {
        let row: Option<(Option<String>, Option<String>, Option<serde_json::Value>)> = sqlx::query_as(
            "SELECT image_key, crop_key, bbox FROM faces WHERE tenant_id = $1 AND id = $2::uuid",
        )
        .bind(tenant_id)
        .bind(&id)
        .fetch_optional(&pool)
        .await
        .ok()
        .flatten();
        row.map(|(k, crop, bbox)| {
            let bb = bbox.and_then(|v| {
                let a = v.as_array()?;
                if a.len() != 4 {
                    return None;
                }
                Some([a[0].as_i64()?, a[1].as_i64()?, a[2].as_i64()?, a[3].as_i64()?])
            });
            (k, crop, bb)
        })
    })
}

/// Backfill pre-cropped thumbnails for faces captured before `crop_key` existed: for up to `limit`
/// rows with a frame + bbox but no crop, fetch the frame, crop it, store the crop, and set
/// `crop_key`. Returns how many it filled. Idempotent — call again for the next batch. Best-effort
/// per row (a fetch/crop/store failure just skips that one).
pub fn backfill_crops(db: &Db, tenant_id: Uuid, limit: i64) -> Result<usize, String> {
    let pool = db.pool();
    let rows: Vec<(String, String, Option<serde_json::Value>)> = db.block(async move {
        sqlx::query_as(
            "SELECT id::text, image_key, bbox FROM faces \
             WHERE tenant_id = $1 AND crop_key IS NULL AND image_key IS NOT NULL AND bbox IS NOT NULL \
             LIMIT $2",
        )
        .bind(tenant_id)
        .bind(limit)
        .fetch_all(&pool)
        .await
        .unwrap_or_default()
    });
    let mut done = 0usize;
    for (id, image_key, bbox) in rows {
        let bb = match bbox.as_ref().and_then(|v| {
            let a = v.as_array()?;
            if a.len() != 4 {
                return None;
            }
            Some([a[0].as_i64()?, a[1].as_i64()?, a[2].as_i64()?, a[3].as_i64()?])
        }) {
            Some(b) => b,
            None => continue,
        };
        let frame = match crate::media::get(&image_key) {
            Ok(b) => b,
            Err(_) => continue,
        };
        let crop = match render_face_image(&frame, bb, false) {
            Some(c) => c,
            None => continue,
        };
        let crop_key = match crate::media::put_face(&crop, "image/jpeg", "jpg") {
            Ok(k) => k,
            Err(_) => continue,
        };
        let pool2 = db.pool();
        let idc = id.clone();
        let kc = crop_key.clone();
        let _ = db.block(async move {
            sqlx::query("UPDATE faces SET crop_key = $1 WHERE tenant_id = $2 AND id = $3::uuid")
                .bind(&kc)
                .bind(tenant_id)
                .bind(&idc)
                .execute(&pool2)
                .await
        });
        done += 1;
    }
    Ok(done)
}

/// Render a stored FRAME for display. The stored image is the whole frame; by default crop it to
/// the face `bbox` (+25% margin) so thumbnails show the face. With `draw_box`, instead return the
/// full frame with the detection rectangle drawn (context for judging far / false-positive faces).
/// Re-encoded JPEG; `None` on decode/encode error → caller serves the raw frame.
pub fn render_face_image(jpeg: &[u8], bbox: [i64; 4], draw_box: bool) -> Option<Vec<u8>> {
    let img = image::load_from_memory(jpeg).ok()?;
    let (iw, ih) = (img.width() as i64, img.height() as i64);
    let [x1, y1, x2, y2] = bbox;
    let mut buf = std::io::Cursor::new(Vec::new());
    if draw_box {
        let mut rgb = img.to_rgb8();
        let color = image::Rgb([90u8, 240, 150]);
        let th = (((x2 - x1).max(y2 - y1)) / 55).clamp(2, 6);
        for t in 0..th {
            for x in x1.max(0)..=x2.min(iw - 1) {
                for &y in &[y1 + t, y2 - t] {
                    if y >= 0 && y < ih {
                        rgb.put_pixel(x as u32, y as u32, color);
                    }
                }
            }
            for y in y1.max(0)..=y2.min(ih - 1) {
                for &x in &[x1 + t, x2 - t] {
                    if x >= 0 && x < iw {
                        rgb.put_pixel(x as u32, y as u32, color);
                    }
                }
            }
        }
        image::DynamicImage::ImageRgb8(rgb).write_to(&mut buf, image::ImageFormat::Jpeg).ok()?;
    } else {
        let (bw, bh) = ((x2 - x1).max(1), (y2 - y1).max(1));
        let (mx, my) = (bw / 4, bh / 4);
        let cx1 = (x1 - mx).clamp(0, iw - 1);
        let cy1 = (y1 - my).clamp(0, ih - 1);
        let cx2 = (x2 + mx).clamp(cx1 + 1, iw);
        let cy2 = (y2 + my).clamp(cy1 + 1, ih);
        img.crop_imm(cx1 as u32, cy1 as u32, (cx2 - cx1) as u32, (cy2 - cy1) as u32)
            .write_to(&mut buf, image::ImageFormat::Jpeg)
            .ok()?;
    }
    Some(buf.into_inner())
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
        // Return BOTH objects (the full frame AND the pre-crop) so the caller purges each.
        let keys: Vec<(Option<String>, Option<String>)> = sqlx::query_as(
            "DELETE FROM faces WHERE tenant_id = $1 AND id = ANY($2::uuid[]) \
             RETURNING image_key, crop_key",
        )
        .bind(tenant_id)
        .bind(&ids)
        .fetch_all(&pool)
        .await
        .map_err(|e| format!("faces::delete: {e}"))?;
        Ok(keys
            .into_iter()
            .flat_map(|(img, crop)| [img, crop])
            .flatten()
            .collect())
    })
}
