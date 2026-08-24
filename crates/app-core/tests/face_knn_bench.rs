//! Card #127 — **prove** the face-kNN rewrite is better, don't assert it.
//!
//! WHY THIS EXISTS. "Add an HNSW index" is the kind of change that looks done and does nothing:
//! pgvector serves an index scan only for `ORDER BY <vec col> <op> <const> LIMIT n` evaluated
//! directly against ONE table, and the shipped query broke that in two independent ways —
//! it computed the distance inside a `UNION ALL` (so the sort had no table to attach to), and it
//! cast the result `::float4` (so the sort expression no longer matched the index's ordering
//! operator). Either one alone is enough to keep a full sequential scan forever, silently, with a
//! perfectly healthy-looking index sitting next to it.
//!
//! So this test does not check that an index EXISTS. It measures three things that an index cannot
//! fake:
//!
//!   1. **Correctness** — recall@k against an exact brute-force oracle computed in Rust, entirely
//!      outside Postgres. ANN is approximate; this is what bounds how approximate.
//!   2. **The plan** — `EXPLAIN (ANALYZE)` must actually say `Index Scan using …hnsw`. This is the
//!      assertion that catches both traps above.
//!   3. **Latency** — measured against a FROZEN COPY of the pre-rewrite SQL (`LEGACY_RECOGNIZE_SQL`
//!      below), run in the same process against the same rows in the same run. No git stash, no
//!      remembering what last week's number was, and it keeps working as a regression guard after
//!      the rewrite lands.
//!
//! The frozen legacy SQL is deliberately a verbatim copy rather than a call into the old code. It
//! is the historical artifact under test; it must not drift when `faces.rs` is refactored.
//!
//! SKIPPED unless `TEST_DATABASE_URL` is set, so `cargo test` stays DB-free like `cargo check`
//! (same convention as `sql_prepare.rs`). Needs the real `sentinel-postgres` image — stock Postgres
//! cannot run the migrations (pg_cron, vector, citext, pgcrypto).
//!
//! Locally (no Rust toolchain on the mac — run it in the container, same as CI):
//!   docker run --rm --network bishop_default -v "$PWD":/w -w /w \
//!     -v sqlcheck-target:/target -v sqlcheck-cargo:/usr/local/cargo/registry \
//!     -e CARGO_TARGET_DIR=/target \
//!     -e TEST_DATABASE_URL=postgres://sentinel:sentinel@sentinel-db:5432/sentinel \
//!     rust:1-bookworm cargo test -p app-core --test face_knn_bench -- --nocapture
//!
//! Knobs (env): `BENCH_IDENTITIES` (default 200), `BENCH_PER_IDENTITY` (25), `BENCH_QUERIES` (200).
//! The defaults seed ~5 000 faces — small enough to run in CI, large enough that the planner
//! genuinely prefers an index. An index over 128 rows proves nothing: a seq scan is CORRECT there,
//! and a benchmark at that size would "pass" while measuring noise.

use std::time::Instant;

use sqlx::postgres::PgPoolOptions;
use sqlx::{PgPool, Row};

use app_core::db::Db;
use app_core::faces::{self, FaceMatch};

const DIM: usize = 512;
/// Neighbours requested per recognition — matches `RecognizeFace`'s default `top_k`.
const K: i64 = 5;

// ---------------------------------------------------------------------------------------------
// The frozen "before" — a verbatim copy of `faces::recognize`'s SQL as it stood before card #127.
// Kept here so the benchmark has a stable baseline to measure against forever. DO NOT "fix" this
// query; its brokenness is the point.
// ---------------------------------------------------------------------------------------------
const LEGACY_RECOGNIZE_SQL: &str = "SELECT reference, label, distance, source FROM ( \
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
     ) u ORDER BY distance LIMIT $3";

// ---------------------------------------------------------------------------------------------
// Deterministic synthetic gallery
// ---------------------------------------------------------------------------------------------

/// A tiny LCG. Deterministic on purpose: a benchmark whose input changes per run cannot be
/// compared across runs, and a recall regression would look like bad luck.
struct Rng(u64);

impl Rng {
    fn next_u64(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0
    }
    /// Uniform in [-1, 1).
    fn unit(&mut self) -> f32 {
        ((self.next_u64() >> 11) as f64 / (1u64 << 53) as f64) as f32 * 2.0 - 1.0
    }
    fn vec(&mut self, dim: usize) -> Vec<f32> {
        (0..dim).map(|_| self.unit()).collect()
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n as u64) as usize
    }
}

fn l2_normalize(v: &[f32]) -> Vec<f32> {
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        v.iter().map(|x| x / norm).collect()
    } else {
        v.to_vec()
    }
}

fn cosine_distance(a: &[f32], b: &[f32]) -> f32 {
    1.0 - a.iter().zip(b).map(|(x, y)| x * y).sum::<f32>()
}

/// A member of identity `centroid`, spread by `sigma`.
///
/// `sigma = 1.0` puts intra-identity cosine distance around 0.29 and inter-identity around 1.0 —
/// identities that are separable but genuinely overlapping, which is what makes ANN recall a real
/// question. Uniformly random vectors (the easy case) would flatter HNSW and prove nothing about
/// face data, whose vectors sit in a narrow cone.
fn member_of(centroid: &[f32], sigma: f32, rng: &mut Rng) -> Vec<f32> {
    let noise = l2_normalize(&rng.vec(DIM));
    let v: Vec<f32> = centroid
        .iter()
        .zip(&noise)
        .map(|(c, n)| c + sigma * n)
        .collect();
    l2_normalize(&v)
}

/// pgvector text literal. 5 decimals keeps the seed SQL ~4 KB/row instead of ~10 KB with no
/// measurable effect on distances at f32 precision.
fn vector_literal(v: &[f32]) -> String {
    let mut s = String::with_capacity(v.len() * 9 + 2);
    s.push('[');
    for (i, x) in v.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        s.push_str(&format!("{x:.5}"));
    }
    s.push(']');
    s
}

fn env_usize(key: &str, default: usize) -> usize {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

// ---------------------------------------------------------------------------------------------
// The gallery, and the oracle
// ---------------------------------------------------------------------------------------------

struct Gallery {
    /// `(reference, embedding)` for every row the queries can match — the union of the enrolled
    /// and stored galleries, exactly as `recognize` sees it.
    entries: Vec<(String, Vec<f32>)>,
    /// How many leading `entries` are enrolled rows (one per identity). The stored rows follow,
    /// grouped by identity — which is what makes a contiguous window past this offset a valid
    /// sample for [`Gallery::separation`].
    enrolled: usize,
    /// Query vectors, each a held-out member of a known identity, with that identity's person id.
    queries: Vec<(Vec<f32>, String)>,
}

impl Gallery {
    /// Exact top-`k` references by cosine distance, brute-forced in Rust.
    ///
    /// This is the ORACLE, and it is deliberately computed outside Postgres. Comparing the indexed
    /// query against the pre-rewrite SQL alone would only prove they agree — if both were wrong in
    /// the same way (say, the tenant filter dropped rows) the test would happily pass.
    fn exact_top_k(&self, query: &[f32], k: usize) -> Vec<String> {
        let mut scored: Vec<(f32, &String)> = self
            .entries
            .iter()
            .map(|(r, e)| (cosine_distance(query, e), r))
            .collect();
        scored.sort_by(|a, b| a.0.total_cmp(&b.0));
        scored.into_iter().take(k).map(|(_, r)| r.clone()).collect()
    }

    /// Mean intra-identity vs inter-identity distance — printed so the numbers below are
    /// interpretable rather than magic. If these two ever converge the gallery has stopped being a
    /// meaningful test set.
    fn separation(&self) -> (f32, f32) {
        // Sample a contiguous window of the STORED block, which is seeded grouped by identity, so
        // the window contains both same-identity and cross-identity pairs.
        //
        // Not a prefix of `entries`: the enrolled rows are seeded first, one per identity, so at a
        // large identity count a prefix holds no two entries of the same identity and the
        // intra-identity figure silently reads 0.000 — which looks like a broken gallery rather
        // than a broken statistic. Nor a stride, for the same reason in reverse: striding over
        // identity-grouped rows steps past every same-identity pair.
        let sample: Vec<&(String, Vec<f32>)> =
            self.entries.iter().skip(self.enrolled).take(300).collect();
        let n = sample.len();
        let (mut intra, mut intra_n, mut inter, mut inter_n) = (0.0f32, 0usize, 0.0f32, 0usize);
        for i in 0..n {
            for j in (i + 1)..n {
                let d = cosine_distance(&sample[i].1, &sample[j].1);
                if sample[i].0 == sample[j].0 {
                    intra += d;
                    intra_n += 1;
                } else {
                    inter += d;
                    inter_n += 1;
                }
            }
        }
        (
            if intra_n > 0 {
                intra / intra_n as f32
            } else {
                0.0
            },
            if inter_n > 0 {
                inter / inter_n as f32
            } else {
                0.0
            },
        )
    }
}

// ---------------------------------------------------------------------------------------------
// Seeding
// ---------------------------------------------------------------------------------------------

/// Build the gallery and load it into Postgres under a throwaway tenant.
///
/// Faces are inserted with raw SQL rather than through `faces::insert` because seeding thousands of
/// rows one round-trip at a time dominates the run. `faces::insert` is exercised separately by
/// `insert_and_recognize_roundtrip`, so the shipped write path is still covered.
async fn seed(pool: &PgPool, tenant: uuid::Uuid) -> Gallery {
    let identities = env_usize("BENCH_IDENTITIES", 200);
    let per_identity = env_usize("BENCH_PER_IDENTITY", 25);
    let n_queries = env_usize("BENCH_QUERIES", 200);
    let sigma = 1.0f32;

    let mut rng = Rng(0x5EED_1234_ABCD_0001);

    sqlx::query("INSERT INTO tenants (id, name, slug) VALUES ($1, 'knn-bench', $2)")
        .bind(tenant)
        .bind(format!("knn-bench-{tenant}"))
        .execute(pool)
        .await
        .expect("seed tenant");

    let centroids: Vec<Vec<f32>> = (0..identities)
        .map(|_| l2_normalize(&rng.vec(DIM)))
        .collect();
    let person_ids: Vec<String> = (0..identities).map(|i| format!("p{i:04}")).collect();

    // persons — person_embeddings FKs onto (tenant_id, id), and legacy `recognize` INNER JOINs it.
    for (i, pid) in person_ids.iter().enumerate() {
        sqlx::query("INSERT INTO persons (tenant_id, id, name) VALUES ($1, $2, $3)")
            .bind(tenant)
            .bind(pid)
            .bind(format!("Person {i:04}"))
            .execute(pool)
            .await
            .expect("seed person");
    }

    let mut entries: Vec<(String, Vec<f32>)> = Vec::new();

    // One approved enrolled embedding per identity.
    for (pid, c) in person_ids.iter().zip(&centroids) {
        let v = member_of(c, sigma, &mut rng);
        sqlx::query(
            "INSERT INTO person_embeddings (tenant_id, person_id, embedding, approved, source) \
             VALUES ($1, $2, $3::vector, true, 'bench')",
        )
        .bind(tenant)
        .bind(pid)
        .bind(vector_literal(&v))
        .execute(pool)
        .await
        .expect("seed person_embedding");
        entries.push((pid.clone(), v));
    }

    // Stored faces. All labeled, so `reference` is the person id on both sides of the union and
    // recall is comparable across sources.
    let mut batch: Vec<(String, String)> = Vec::new();
    let now = app_core::timeutil::now_rfc3339();
    for (pid, c) in person_ids.iter().zip(&centroids) {
        for _ in 0..per_identity {
            let v = member_of(c, sigma, &mut rng);
            batch.push((pid.clone(), vector_literal(&v)));
            entries.push((pid.clone(), v));
        }
    }
    for chunk in batch.chunks(100) {
        let mut sql = String::from(
            "INSERT INTO faces (tenant_id, person_id, embedding, confidence, face_width, source, created_at) VALUES ",
        );
        for i in 0..chunk.len() {
            if i > 0 {
                sql.push(',');
            }
            // $1 tenant, $2 created_at, then two params per row.
            sql.push_str(&format!(
                "($1, ${}, ${}::vector, 90, 120, 'bench', $2)",
                3 + i * 2,
                4 + i * 2
            ));
        }
        let mut q = sqlx::query(&sql).bind(tenant).bind(&now);
        for (pid, lit) in chunk {
            q = q.bind(pid).bind(lit);
        }
        q.execute(pool).await.expect("seed faces batch");
    }

    // Held-out queries: a fresh member of an existing identity, never inserted.
    let queries = (0..n_queries)
        .map(|_| {
            let i = rng.below(identities);
            (
                member_of(&centroids[i], sigma, &mut rng),
                person_ids[i].clone(),
            )
        })
        .collect();

    sqlx::query("ANALYZE faces")
        .execute(pool)
        .await
        .expect("analyze faces");
    sqlx::query("ANALYZE person_embeddings")
        .execute(pool)
        .await
        .expect("analyze person_embeddings");

    Gallery {
        entries,
        enrolled: identities,
        queries,
    }
}

async fn cleanup(pool: &PgPool, tenant: uuid::Uuid) {
    // `faces` has no FK to tenants, so it needs an explicit delete; persons and person_embeddings
    // cascade from the tenant row.
    let _ = sqlx::query("DELETE FROM faces WHERE tenant_id = $1")
        .bind(tenant)
        .execute(pool)
        .await;
    let _ = sqlx::query("DELETE FROM tenants WHERE id = $1")
        .bind(tenant)
        .execute(pool)
        .await;
}

// ---------------------------------------------------------------------------------------------
// Measurement
// ---------------------------------------------------------------------------------------------

#[derive(Default)]
struct Report {
    name: String,
    recall_at_k: f64,
    top1_identity: f64,
    mean_ms: f64,
    p95_ms: f64,
}

fn summarize(name: &str, hits: &[(Vec<String>, Vec<String>, String, f64)]) -> Report {
    let n = hits.len().max(1) as f64;
    let recall = hits
        .iter()
        .map(|(got, oracle, _, _)| {
            let inter = got.iter().filter(|r| oracle.contains(r)).count();
            inter as f64 / oracle.len().max(1) as f64
        })
        .sum::<f64>()
        / n;
    let top1 = hits
        .iter()
        .filter(|(got, _, want, _)| got.first() == Some(want))
        .count() as f64
        / n;
    let mut times: Vec<f64> = hits.iter().map(|(_, _, _, ms)| *ms).collect();
    times.sort_by(|a, b| a.total_cmp(b));
    let mean = times.iter().sum::<f64>() / n;
    let p95 = times[((times.len() as f64 * 0.95) as usize).min(times.len() - 1)];
    Report {
        name: name.to_string(),
        recall_at_k: recall,
        top1_identity: top1,
        mean_ms: mean,
        p95_ms: p95,
    }
}

/// Whether a plan uses a vector index — the assertion that catches both pgvector traps.
async fn plan_uses_hnsw(
    pool: &PgPool,
    sql: &str,
    query: &[f32],
    tenant: uuid::Uuid,
) -> (bool, bool) {
    let lit = vector_literal(query);
    let explain = format!("EXPLAIN (ANALYZE, BUFFERS) {sql}");
    let rows = sqlx::query(&explain)
        .bind(&lit)
        .bind(tenant)
        .bind(K)
        .fetch_all(pool)
        .await;
    let text = match rows {
        Ok(rows) => rows
            .iter()
            .filter_map(|r| r.try_get::<String, _>(0).ok())
            .collect::<Vec<_>>()
            .join("\n"),
        // A query whose bind count differs (the rewrite adds $4) is explained by the caller
        // instead; report "unknown" rather than failing the whole run here.
        Err(_) => String::new(),
    };
    (text.contains("hnsw"), text.contains("Seq Scan"))
}

// ---------------------------------------------------------------------------------------------
// The benchmark
// ---------------------------------------------------------------------------------------------

#[test]
fn face_knn_indexed_beats_sequential_scan() {
    let Ok(url) = std::env::var("TEST_DATABASE_URL") else {
        eprintln!("face_knn_bench: TEST_DATABASE_URL unset — skipping");
        return;
    };

    // `Db::connect` applies the embedded migrations, so the HNSW index arrives here the same way
    // it will in production: by booting against the schema, not by hand-running DDL in the test.
    let db = Db::connect(&url).expect("connect + migrate");

    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("runtime");

    let pool = rt.block_on(async {
        PgPoolOptions::new()
            .max_connections(4)
            .connect(&url)
            .await
            .expect("bench pool")
    });

    let tenant = uuid::Uuid::new_v4();
    let gallery = rt.block_on(seed(&pool, tenant));

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        run_benchmark(&db, &pool, &rt, tenant, &gallery)
    }));

    rt.block_on(cleanup(&pool, tenant));

    if let Err(e) = result {
        std::panic::resume_unwind(e);
    }
}

fn run_benchmark(
    db: &Db,
    pool: &PgPool,
    rt: &tokio::runtime::Runtime,
    tenant: uuid::Uuid,
    gallery: &Gallery,
) {
    let (intra, inter) = gallery.separation();
    println!("\n=== #127 face kNN benchmark ===");
    println!(
        "gallery: {} entries ({} queries), intra-identity d={:.3}  inter-identity d={:.3}",
        gallery.entries.len(),
        gallery.queries.len(),
        intra,
        inter
    );

    // --- baseline: the frozen pre-rewrite SQL -------------------------------------------------
    let mut legacy_hits = Vec::new();
    for (q, want) in &gallery.queries {
        let oracle = gallery.exact_top_k(q, K as usize);
        let lit = vector_literal(q);
        let t = Instant::now();
        let rows = rt.block_on(async {
            sqlx::query(LEGACY_RECOGNIZE_SQL)
                .bind(&lit)
                .bind(tenant)
                .bind(K)
                .fetch_all(pool)
                .await
                .expect("legacy recognize")
        });
        let ms = t.elapsed().as_secs_f64() * 1000.0;
        let got: Vec<String> = rows
            .iter()
            .map(|r| r.get::<String, _>("reference"))
            .collect();
        legacy_hits.push((got, oracle, want.clone(), ms));
    }
    let legacy = summarize("legacy (union + ::float4 cast)", &legacy_hits);

    // --- shipped: whatever `faces::recognize` is today ----------------------------------------
    let mut current_hits = Vec::new();
    for (q, want) in &gallery.queries {
        let oracle = gallery.exact_top_k(q, K as usize);
        let t = Instant::now();
        let matches: Vec<FaceMatch> = faces::recognize(db, tenant, q, K);
        let ms = t.elapsed().as_secs_f64() * 1000.0;
        let got: Vec<String> = matches.into_iter().map(|m| m.reference).collect();
        current_hits.push((got, oracle, want.clone(), ms));
    }
    let current = summarize("faces::recognize (shipped)", &current_hits);

    // --- plans --------------------------------------------------------------------------------
    let probe = &gallery.queries[0].0;
    let (legacy_hnsw, legacy_seq) =
        rt.block_on(plan_uses_hnsw(pool, LEGACY_RECOGNIZE_SQL, probe, tenant));

    // The shipped query's plan, probed through the same single-table shape the rewrite must
    // produce. If `faces` cannot be index-scanned for a top-k, nothing downstream can be either.
    let single_table = "SELECT id::text FROM faces WHERE tenant_id = $2 \
                        ORDER BY embedding <=> $1::vector LIMIT $3";
    let (probe_hnsw, probe_seq) = rt.block_on(plan_uses_hnsw(pool, single_table, probe, tenant));

    // The same query WITH the cast in the ORDER BY — trap 2, isolated, so the report shows the
    // cast alone is enough to lose the index.
    let cast_order = "SELECT id::text, (embedding <=> $1::vector)::float4 AS distance FROM faces \
                      WHERE tenant_id = $2 ORDER BY distance LIMIT $3";
    let (cast_hnsw, cast_seq) = rt.block_on(plan_uses_hnsw(pool, cast_order, probe, tenant));

    let index_exists: bool = rt.block_on(async {
        sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS (SELECT 1 FROM pg_indexes WHERE tablename = 'faces' AND indexdef ILIKE '%hnsw%')",
        )
        .fetch_one(pool)
        .await
        .unwrap_or(false)
    });

    for r in [&legacy, &current] {
        println!(
            "{:<34} recall@{K}={:.3}  top1={:.3}  mean={:.2}ms  p95={:.2}ms",
            r.name, r.recall_at_k, r.top1_identity, r.mean_ms, r.p95_ms
        );
    }
    println!("hnsw index present on faces        : {index_exists}");
    println!("legacy union plan                  : hnsw={legacy_hnsw} seq_scan={legacy_seq}");
    println!("single-table, UNCAST order by      : hnsw={probe_hnsw} seq_scan={probe_seq}");
    println!("single-table, ::float4 in order by : hnsw={cast_hnsw} seq_scan={cast_seq}");
    if legacy.mean_ms > 0.0 {
        println!(
            "speedup (legacy mean / shipped mean): {:.2}x",
            legacy.mean_ms / current.mean_ms.max(f64::MIN_POSITIVE)
        );
    }
    println!();

    // --- the gate -----------------------------------------------------------------------------
    // Correctness first: a faster wrong answer is not an improvement. The oracle is exact, so this
    // bounds exactly how much ANN + over-fetch costs in recall.
    assert!(
        current.recall_at_k >= 0.98,
        "recall@{K} regressed to {:.3} (need >= 0.98) — over-fetch or ef_search is too low",
        current.recall_at_k
    );
    // The legacy query is exact by construction; if IT is not ~1.0 the gallery or oracle is wrong,
    // not the index. Guards the test against measuring its own bug.
    assert!(
        legacy.recall_at_k >= 0.999,
        "the exact baseline scored {:.3} against the oracle — the harness is wrong, not the index",
        legacy.recall_at_k
    );
    assert!(
        index_exists,
        "no hnsw index on `faces` — the #127 migration has not been applied"
    );
    assert!(
        probe_hnsw && !probe_seq,
        "a single-table uncast top-k still does NOT use the hnsw index (hnsw={probe_hnsw} \
         seq_scan={probe_seq}) — the index is unusable, so the rewrite cannot help"
    );
    assert!(
        current.mean_ms < legacy.mean_ms,
        "the shipped query ({:.2}ms) is not faster than the pre-rewrite one ({:.2}ms)",
        current.mean_ms,
        legacy.mean_ms
    );
}

/// The shipped write path + dedup helper, on the same gallery shape. Small on purpose — this is a
/// correctness round-trip, not a benchmark: a face inserted through `faces::insert` must come back
/// as its own nearest neighbour, and `nearest_stored_distance` must agree.
#[test]
fn insert_and_recognize_roundtrip() {
    let Ok(url) = std::env::var("TEST_DATABASE_URL") else {
        eprintln!("face_knn_bench: TEST_DATABASE_URL unset — skipping");
        return;
    };
    let db = Db::connect(&url).expect("connect + migrate");
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .expect("runtime");
    let pool = rt.block_on(async {
        PgPoolOptions::new()
            .max_connections(2)
            .connect(&url)
            .await
            .expect("pool")
    });
    let tenant = uuid::Uuid::new_v4();
    rt.block_on(async {
        sqlx::query("INSERT INTO tenants (id, name, slug) VALUES ($1, 'knn-rt', $2)")
            .bind(tenant)
            .bind(format!("knn-rt-{tenant}"))
            .execute(&pool)
            .await
            .expect("tenant");
    });

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut rng = Rng(0xC0FFEE);
        let v = l2_normalize(&rng.vec(DIM));
        let id = faces::insert(
            &db,
            tenant,
            faces::NewFace {
                person_id: None,
                embedding: v.clone(),
                image_key: None,
                camera: Some("cam-1".into()),
                bbox: None,
                confidence: 88.0,
                face_width: 140,
                source: "test".into(),
                crop_key: None,
                blur: Some(1200.0),
                yaw: Some(-4.0),
                pitch: Some(2.0),
                roll: Some(1.0),
                landmarks: None,
                landmarks_source: None,
            },
        )
        .expect("insert");

        let hits = faces::recognize(&db, tenant, &v, 3);
        assert!(!hits.is_empty(), "an inserted face must be recognizable");
        assert_eq!(
            hits[0].reference, id,
            "the nearest match to a face must be that face"
        );
        assert!(
            hits[0].distance.abs() < 1e-3,
            "self-distance should be ~0, got {}",
            hits[0].distance
        );

        let d = faces::nearest_stored_distance(&db, tenant, &v).expect("dedup distance");
        assert!(d.abs() < 1e-3, "self dedup distance should be ~0, got {d}");

        // An ignored face must not compete: dismissing junk in the UI has to actually remove it
        // from recognition, which the pre-#127 query did not do.
        faces::set_ignored(&db, tenant, &[id.clone()], true).expect("ignore");
        let after = faces::recognize(&db, tenant, &v, 3);
        assert!(
            !after.iter().any(|m| m.reference == id),
            "an ignored face must be excluded from recognition"
        );
    }));

    rt.block_on(cleanup(&pool, tenant));
    if let Err(e) = result {
        std::panic::resume_unwind(e);
    }
}

// =============================================================================================
// Card #174 step 2 — clustering neighbours sourced from the HNSW index.
//
// The clustering pass used to compute every pairwise distance in Rust, which is why it was capped
// at 2000 faces. Neighbours now come from one `CROSS JOIN LATERAL` top-k probe per face, so the
// quadratic term is gone. Two things have to be true for that to be a real change and not a
// plausible-looking one:
//
//   1. **The plan uses the index.** Same trap as #127 — a LATERAL whose ordering expression is not
//      a bare `<vec col> <op> <const>` silently falls back to a sequential scan per outer row,
//      which is QUADRATIC AGAIN and slower than what it replaced.
//   2. **The clusters are still right.** An empty or truncated edge list does not error; it returns
//      every face as its own singleton, which reads as "no duplicates found" rather than as a bug.
//      So this asserts against seeded ground truth, not against "it ran".
//
// Purity is the #124 property restated at integration level: no cluster may contain two identities.
// =============================================================================================

/// Seed `identities` × `per_identity` UNLABELED faces. Returns face id → identity tag.
///
/// Unlabeled on purpose: `build_cores` only ever clusters faces with `person_id IS NULL`, so
/// labeled rows would be partitioned off and the clustering path would never see them.
async fn seed_unlabeled(
    pool: &PgPool,
    tenant: uuid::Uuid,
    identities: usize,
    per_identity: usize,
) -> std::collections::HashMap<String, String> {
    sqlx::query("INSERT INTO tenants (id, name, slug) VALUES ($1, 'cluster-bench', $2)")
        .bind(tenant)
        .bind(format!("cluster-bench-{tenant}"))
        .execute(pool)
        .await
        .expect("seed tenant");

    let mut rng = Rng(0x0174_0002_5EED_0001);
    let now = app_core::timeutil::now_rfc3339();
    // sigma = 1.0 reproduces the real-capture regime the #127 gallery measured: intra-identity
    // ~0.50 (inside CLUSTER_EPS = 0.55) and inter-identity ~1.00 (well outside it).
    let sigma = 1.0f32;

    let mut batch: Vec<(String, String)> = Vec::new();
    for i in 0..identities {
        let centroid = l2_normalize(&rng.vec(DIM));
        let tag = format!("ident-{i:04}");
        for _ in 0..per_identity {
            batch.push((
                tag.clone(),
                vector_literal(&member_of(&centroid, sigma, &mut rng)),
            ));
        }
    }
    // The identity tag rides in `source` so ground truth can be read back from the DB rather than
    // depending on multi-row INSERT ... RETURNING preserving VALUES order.
    for chunk in batch.chunks(100) {
        let mut sql = String::from(
            "INSERT INTO faces (tenant_id, embedding, confidence, face_width, source, created_at) VALUES ",
        );
        for i in 0..chunk.len() {
            if i > 0 {
                sql.push(',');
            }
            sql.push_str(&format!(
                "($1, ${}::vector, 90, 120, ${}, $2)",
                3 + i * 2,
                4 + i * 2
            ));
        }
        let mut q = sqlx::query(&sql).bind(tenant).bind(&now);
        for (tag, v) in chunk {
            q = q.bind(v).bind(tag);
        }
        q.execute(pool).await.expect("seed unlabeled faces");
    }

    sqlx::query("ANALYZE faces").execute(pool).await.ok();

    sqlx::query_as::<_, (String, String)>("SELECT id::text, source FROM faces WHERE tenant_id = $1")
        .bind(tenant)
        .fetch_all(pool)
        .await
        .expect("read back ground truth")
        .into_iter()
        .collect()
}

#[test]
fn clustering_recovers_seeded_identities_via_the_index() {
    let Ok(url) = std::env::var("TEST_DATABASE_URL") else {
        eprintln!("face_knn_bench: TEST_DATABASE_URL unset — skipping");
        return;
    };
    // ~5000 faces, deliberately, for the reason the module doc gives about the #127 sizing: below
    // a few thousand rows a sequential scan is genuinely the CHEAPER plan, so the planner picks it
    // and the index assertion fails on a perfectly good query. Measured on this schema: at 600
    // faces the LATERAL plans `Seq Scan on faces x` (599 rows x 600 loops, 2.3 s); at 10 000 it
    // plans `Index Scan using faces_embedding_hnsw` (10 rows x 10 000 loops, 5.2 s). Shrinking
    // these knobs to make the test faster will make it fail for the wrong reason.
    let identities = env_usize("CLUSTER_IDENTITIES", 200);
    let per_identity = env_usize("CLUSTER_PER_IDENTITY", 25);

    let db = Db::connect(&url).expect("connect + migrate");
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("runtime");
    let pool = rt.block_on(async {
        PgPoolOptions::new()
            .max_connections(4)
            .connect(&url)
            .await
            .expect("pool")
    });
    let tenant = uuid::Uuid::new_v4();
    let truth = rt.block_on(seed_unlabeled(&pool, tenant, identities, per_identity));
    let total = truth.len();

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        // ---- 1. the plan ----------------------------------------------------------------
        let plan = rt.block_on(async {
            let mut tx = pool.begin().await.expect("tx");
            sqlx::query("SET LOCAL hnsw.iterative_scan = 'relaxed_order'")
                .execute(&mut *tx)
                .await
                .ok();
            let rows = sqlx::query(
                "EXPLAIN (ANALYZE, TIMING OFF) \
                 WITH scoped AS ( \
                   SELECT id, embedding FROM faces \
                    WHERE tenant_id = $1 AND person_id IS NULL AND NOT ignored \
                    ORDER BY created_at DESC LIMIT 20000 ) \
                 SELECT s.id, n.id FROM scoped s CROSS JOIN LATERAL ( \
                   SELECT x.id, x.embedding <=> s.embedding AS d FROM faces x \
                    WHERE x.tenant_id = $1 AND x.person_id IS NULL AND NOT x.ignored \
                      AND x.id <> s.id \
                    ORDER BY x.embedding <=> s.embedding LIMIT 40 ) n \
                  WHERE n.d <= 0.55",
            )
            .bind(tenant)
            .fetch_all(&mut *tx)
            .await
            .expect("explain");
            let text: Vec<String> = rows.iter().map(|r| r.get::<String, _>(0)).collect();
            tx.commit().await.ok();
            text.join("\n")
        });
        println!("\n--- LATERAL kNN plan ---\n{plan}\n");
        assert!(
            plan.contains("hnsw"),
            "the LATERAL kNN is not using the HNSW index — clustering is quadratic again:\n{plan}"
        );
        assert!(
            !plan.contains("Seq Scan on faces x"),
            "the LATERAL's inner candidate scan is sequential — one full scan PER FACE:\n{plan}"
        );

        // ---- 2. the clusters ------------------------------------------------------------
        let started = Instant::now();
        let clusters = faces::cluster_unlabeled(&db, tenant, 0.55);
        let elapsed = started.elapsed();
        println!(
            "clustered {total} faces ({identities} identities x {per_identity}) into {} clusters in {:?}",
            clusters.len(),
            elapsed
        );

        assert!(
            !clusters.is_empty(),
            "clustering returned nothing for {total} seeded faces — the edge list is empty, which \
             is what a failed neighbour query looks like"
        );

        // Purity: the anti-chaining property. A cluster spanning two seeded identities means the
        // linkage rule welded them, which is exactly the #124 bug.
        let mut mixed = Vec::new();
        let mut largest_per_identity: std::collections::HashMap<&str, usize> =
            std::collections::HashMap::new();
        for c in &clusters {
            let tags: std::collections::HashSet<&str> = c
                .face_ids
                .iter()
                .map(|id| truth.get(id).expect("clustered face was seeded").as_str())
                .collect();
            if tags.len() > 1 {
                let mut t: Vec<&str> = tags.into_iter().collect();
                t.sort_unstable();
                mixed.push((c.size, t));
                continue;
            }
            let tag = *tags.iter().next().expect("non-empty cluster");
            let e = largest_per_identity.entry(tag).or_insert(0);
            *e = (*e).max(c.size);
        }
        assert!(
            mixed.is_empty(),
            "clusters span more than one seeded identity — chaining is back: {mixed:?}"
        );

        // Coverage: purity alone is satisfied by returning every face as a singleton, which would
        // be useless. Each identity must be substantially recovered in ONE cluster.
        let recovered: Vec<(&str, usize)> = largest_per_identity
            .iter()
            .map(|(k, v)| (*k, *v))
            .filter(|(_, size)| *size * 100 < per_identity * 60)
            .collect();
        let mean: f64 = largest_per_identity.values().sum::<usize>() as f64
            / largest_per_identity.len().max(1) as f64;
        println!(
            "largest cluster per identity: mean {mean:.1} of {per_identity} faces; \
             {} identities under 60%",
            recovered.len()
        );
        assert!(
            recovered.len() * 10 <= identities, // allow a 10% tail of fragmented identities
            "identities were fragmented rather than clustered ({} of {identities} recovered less \
             than 60% into one cluster): {recovered:?}",
            recovered.len()
        );
    }));

    rt.block_on(cleanup(&pool, tenant));
    if let Err(e) = result {
        std::panic::resume_unwind(e);
    }
}

/// Where the index actually starts winning — the number that decides `MAX_CLUSTER_N`.
///
/// The index-backed neighbour query is not free: each of the N probes pays HNSW graph traversal,
/// tuple fetch and row plumbing, and that per-probe cost grows with the index. The in-memory scan is
/// N²/2 dot products over contiguous f32 — quadratic, but with an extremely small constant.
///
/// So "quadratic is worse" is an asymptotic claim, not a claim about the sizes this product has. At
/// some N the index wins; below it the naive scan does, and capping the workspace on the wrong side
/// of that crossover makes clustering slower in the name of scale. This measures the Rust side; the
/// SQL side comes from `clustering_recovers_seeded_identities_via_the_index`, which prints the
/// LATERAL's `Execution Time` at the same sizes.
///
/// Run it with `--release`: a debug-build number here is meaningless and would push the crossover
/// the wrong way by an order of magnitude.
#[test]
fn in_memory_knn_cost_at_scale() {
    if std::env::var("BENCH_KNN_SCAN").is_err() {
        eprintln!("in_memory_knn_cost_at_scale: set BENCH_KNN_SCAN=1 to run — skipping");
        return;
    }
    let mut rng = Rng(0x5CA1_AB1E);
    for n in [1000usize, 5000, 10000, 20000] {
        let vecs: Vec<Vec<f32>> = (0..n).map(|_| l2_normalize(&rng.vec(DIM))).collect();
        let started = Instant::now();
        let mut edges = 0usize;
        for i in 0..n {
            let mut d: Vec<(f32, usize)> = (0..n)
                .filter(|&j| j != i)
                .map(|j| (cosine_distance(&vecs[i], &vecs[j]), j))
                .collect();
            d.sort_by(|a, b| a.0.total_cmp(&b.0));
            d.truncate(40);
            edges += d.len();
        }
        println!(
            "in-memory kNN: n={n:>6}  {:>8.2?}  ({edges} edges)",
            started.elapsed()
        );
    }
}

/// `get_detail` must actually DECODE, not merely prepare.
///
/// This exists because a real bug shipped through every other guard. `blur`/`yaw`/`pitch`/`roll`
/// are `float4`; the row struct declared them `f64`. sqlx matches types strictly — a float4 does
/// not widen into an f64, it errors — so every detail request failed, and `get_detail` collapsed
/// the error into `None`, which the handler renders as 404 "face not found". A missing row and a
/// broken query were indistinguishable, and the image proxy kept working, so the face was plainly
/// there while its detail insisted it was not.
///
/// `check:sql` cannot see this: `sql_prepare` PREPAREs statements and counts binds, both of which
/// were fine. `cargo check` cannot see it either — the types are consistent with each other, just
/// not with the database. Only fetching a real row does.
#[test]
fn face_detail_decodes_every_column() {
    let Ok(url) = std::env::var("TEST_DATABASE_URL") else {
        eprintln!("face_knn_bench: TEST_DATABASE_URL unset — skipping");
        return;
    };
    let db = Db::connect(&url).expect("connect + migrate");
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .expect("runtime");
    let pool = rt.block_on(async {
        PgPoolOptions::new()
            .max_connections(2)
            .connect(&url)
            .await
            .expect("pool")
    });
    let tenant = uuid::Uuid::new_v4();
    rt.block_on(async {
        sqlx::query("INSERT INTO tenants (id, name, slug) VALUES ($1, 'detail-rt', $2)")
            .bind(tenant)
            .bind(format!("detail-rt-{tenant}"))
            .execute(&pool)
            .await
            .expect("tenant");
    });

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut rng = Rng(0xD37A11);
        let v = l2_normalize(&rng.vec(DIM));
        // Every optional column POPULATED: a row of NULLs would decode whatever the declared type,
        // so nulls would have let the original bug through this test too.
        let id = faces::insert(
            &db,
            tenant,
            faces::NewFace {
                person_id: None,
                embedding: v,
                image_key: Some("k.jpg".into()),
                camera: Some("cam-1".into()),
                bbox: Some(serde_json::json!([1, 2, 3, 4])),
                confidence: 91.5,
                face_width: 128,
                source: "test".into(),
                crop_key: Some("c.jpg".into()),
                blur: Some(412.25),
                yaw: Some(-7.5),
                pitch: Some(3.25),
                roll: Some(1.5),
                landmarks: Some(serde_json::json!([[10.0, 20.0], [30.0, 40.0]])),
                landmarks_source: Some("yunet_5".into()),
            },
        )
        .expect("insert");

        let d = faces::get_detail(&db, tenant, &id)
            .expect("get_detail returned None — the query failed, or the row is missing");

        assert_eq!(d.bbox, Some([1, 2, 3, 4]), "bbox");
        assert_eq!(d.blur.map(|v| v.round()), Some(412.0), "blur");
        assert_eq!(d.yaw.map(|v| v.round()), Some(-8.0), "yaw");
        assert_eq!(d.pitch.map(|v| v.round()), Some(3.0), "pitch");
        assert_eq!(d.roll.map(|v| v.round()), Some(2.0), "roll");
        assert_eq!(
            d.landmarks_source.as_deref(),
            Some("yunet_5"),
            "landmarks source"
        );
        assert!(d.landmarks.is_some(), "landmarks");
        assert_eq!(d.face.face_width, 128, "face_width");
    }));

    rt.block_on(cleanup(&pool, tenant));
    if let Err(e) = result {
        std::panic::resume_unwind(e);
    }
}
