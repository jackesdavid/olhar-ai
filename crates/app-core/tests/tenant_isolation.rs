//! Card #160 Phase 1 — **prove** how much cross-tenant isolation we actually have.
//!
//! WHY THIS EXISTS. Isolation today is enforced by writing `tenant_id = $n` by hand in 330 sqlx
//! call sites. Nothing checks it: `check:rust` cannot (the queries are runtime strings), and
//! `check:sql` verifies every statement PREPAREs and that bind counts match but has **no opinion
//! about whether a tenant predicate is present**. A query that returns every tenant's rows is, to
//! every gate this repository owns, a perfectly healthy query.
//!
//! So this test seeds TWO tenants with deliberately overlapping data, drives the REAL repository
//! functions as tenant A, and asserts that nothing belonging to tenant B is ever returned.
//!
//! ## The negative control is the important part
//!
//! A test like this passes trivially if the seed data does not actually overlap, or if the "other
//! tenant" rows were never written. It would then pass forever, including on the day isolation
//! breaks — the same way `the_dot_field_mark_is_present_with_radial_falloff` kept passing for a
//! logo the brand had already retired, and the way an HNSW index sat unused behind a cast while
//! every test was green.
//!
//! [`negative_control_a_query_without_the_tenant_predicate_sees_both`] therefore runs a
//! deliberately tenant-less query and **requires** it to see both tenants. If that control ever
//! fails, the fixtures are wrong and every other assertion in this file is vacuous.
//!
//! ## What a failure here means
//!
//! A failure is a real cross-tenant read: one customer's account returning another customer's
//! camera data. Treat it as an incident, not a flaky test.
//!
//! SKIPPED unless `TEST_DATABASE_URL` is set, so `cargo test` stays DB-free (same convention as
//! `sql_prepare.rs`). Needs the real `sentinel-postgres` image — the migrations need pg_cron,
//! vector, citext and pgcrypto.
//!
//! Locally (no Rust toolchain on the mac — run it in the container, same as CI):
//!   docker run --rm --network bishop_default -v "$PWD":/w -w /w \
//!     -v gitlab-cargo-target:/target -v gitlab-cargo-registry:/usr/local/cargo/registry \
//!     -e CARGO_TARGET_DIR=/target \
//!     -e TEST_DATABASE_URL=postgres://sentinel:sentinel@sentinel-db:5432/sentinel \
//!     rust:1-bookworm cargo test -p app-core --test tenant_isolation -- --nocapture

use sqlx::postgres::PgPoolOptions;
use sqlx::{PgPool, Row};

use app_core::db::Db;
use app_core::{faces, persons};

const DIM: usize = 512;

/// Two tenants and the rows planted under each, so assertions can name what should be invisible.
struct Fixture {
    a: uuid::Uuid,
    b: uuid::Uuid,
    /// Face ids owned by B — none of these may ever surface while acting as A.
    b_face_ids: Vec<String>,
    /// Person id owned by B.
    b_person: String,
}

fn l2_normalize(v: &[f32]) -> Vec<f32> {
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        v.iter().map(|x| x / norm).collect()
    } else {
        v.to_vec()
    }
}

/// A deterministic unit vector. Both tenants get vectors from the SAME small region on purpose: if
/// A's and B's embeddings were far apart, a kNN search would avoid B's rows by distance alone and
/// the isolation assertion would prove nothing.
fn vector(seed: u64) -> Vec<f32> {
    let mut x = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
    let mut v = Vec::with_capacity(DIM);
    for i in 0..DIM {
        x = x.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        // A large shared component plus a small per-seed wobble → all vectors near-parallel.
        let wobble = ((x >> 33) as f64 / (1u64 << 31) as f64) as f32 - 0.5;
        v.push(1.0 + 0.02 * wobble * (i % 7) as f32);
    }
    l2_normalize(&v)
}

async fn seed(pool: &PgPool, db: &Db) -> Fixture {
    let a = uuid::Uuid::new_v4();
    let b = uuid::Uuid::new_v4();

    for (t, name) in [(a, "iso-a"), (b, "iso-b")] {
        sqlx::query("INSERT INTO tenants (id, name, slug) VALUES ($1, $2, $3)")
            .bind(t)
            .bind(name)
            .bind(format!("{name}-{t}"))
            .execute(pool)
            .await
            .expect("seed tenant");
    }

    // A person in each tenant, deliberately sharing the same `id` string. `persons`' PK is
    // (tenant_id, id), so this is legal — and it is the sharpest possible probe: a query that
    // filters on `id` but forgets `tenant_id` returns the wrong customer's person.
    let shared_person_id = "p-shared";
    for (t, label) in [(a, "Alice of A"), (b, "Bob of B")] {
        sqlx::query("INSERT INTO persons (tenant_id, id, name) VALUES ($1, $2, $3)")
            .bind(t)
            .bind(shared_person_id)
            .bind(label)
            .execute(pool)
            .await
            .expect("seed person");
    }

    let mut b_face_ids = Vec::new();
    for (i, (t, who)) in [(a, "A"), (b, "B")].into_iter().enumerate() {
        for n in 0..4 {
            let id = faces::insert(
                db,
                t,
                faces::NewFace {
                    person_id: Some(shared_person_id.to_string()),
                    embedding: vector((i * 100 + n) as u64),
                    image_key: Some(format!("{who}-image-{n}")),
                    camera: Some(format!("{who}-camera")),
                    bbox: None,
                    confidence: 90.0,
                    face_width: 120,
                    source: format!("iso-{who}"),
                    crop_key: None,
                    blur: Some(1000.0),
                    yaw: Some(0.0),
                    pitch: Some(0.0),
                    roll: Some(0.0),
                },
            )
            .expect("seed face");
            if t == b {
                b_face_ids.push(id);
            }
        }
    }

    Fixture {
        a,
        b,
        b_face_ids,
        b_person: shared_person_id.to_string(),
    }
}

async fn cleanup(pool: &PgPool, f: &Fixture) {
    for t in [f.a, f.b] {
        let _ = sqlx::query("DELETE FROM faces WHERE tenant_id = $1")
            .bind(t)
            .execute(pool)
            .await;
        let _ = sqlx::query("DELETE FROM tenants WHERE id = $1")
            .bind(t)
            .execute(pool)
            .await;
    }
}

fn setup() -> Option<(Db, PgPool, tokio::runtime::Runtime, Fixture)> {
    let url = std::env::var("TEST_DATABASE_URL").ok()?;
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
    let fixture = rt.block_on(seed(&pool, &db));
    Some((db, pool, rt, fixture))
}

/// Run `body`, then always clean up, then re-raise any panic. Without this a failing assertion
/// leaves two tenants and eight faces behind, and the next run's fixtures collide.
fn with_fixture(f: impl FnOnce(&Db, &PgPool, &tokio::runtime::Runtime, &Fixture)) {
    let Some((db, pool, rt, fixture)) = setup() else {
        eprintln!("tenant_isolation: TEST_DATABASE_URL unset — skipping");
        return;
    };
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        f(&db, &pool, &rt, &fixture)
    }));
    rt.block_on(cleanup(&pool, &fixture));
    if let Err(e) = result {
        std::panic::resume_unwind(e);
    }
}

// ---------------------------------------------------------------------------------------------
// The negative control — run this first, mentally. Everything else depends on it.
// ---------------------------------------------------------------------------------------------

/// **If this test fails, every other test in this file is meaningless.**
///
/// It runs the same shape of query the repository runs, minus the tenant predicate, and requires
/// it to see BOTH tenants. That proves two things the other assertions silently assume: the
/// fixtures really do plant overlapping rows, and a leak of this exact shape is detectable here.
#[test]
fn negative_control_a_query_without_the_tenant_predicate_sees_both() {
    with_fixture(|_db, pool, rt, _f| {
        let rows: Vec<(String,)> = rt.block_on(async {
            // Deliberately tenant-less. This is the bug the card exists to prevent, written on
            // purpose so the harness can prove it would notice.
            sqlx::query_as("SELECT source FROM faces WHERE source IN ('iso-A', 'iso-B')")
                .fetch_all(pool)
                .await
                .expect("control query")
        });
        let sources: std::collections::HashSet<&str> =
            rows.iter().map(|(s,)| s.as_str()).collect();
        assert!(
            sources.contains("iso-A") && sources.contains("iso-B"),
            "the fixtures do not overlap across tenants (saw {sources:?}) — every isolation \
             assertion in this file would pass vacuously. Fix the fixtures, not this test."
        );
        println!("negative control OK: a tenant-less query sees both tenants, as it must");
    });
}

// ---------------------------------------------------------------------------------------------
// The real assertions
// ---------------------------------------------------------------------------------------------

/// Listing faces as A must never surface B's captures. `faces` holds biometric data and the face
/// crops it was computed from — the most sensitive table in the product.
#[test]
fn listing_faces_never_returns_another_tenants_captures() {
    with_fixture(|db, _pool, _rt, f| {
        let listed = faces::list(db, f.a, 500);
        let leaked: Vec<&str> = listed
            .iter()
            .filter(|face| f.b_face_ids.contains(&face.id))
            .map(|face| face.id.as_str())
            .collect();
        assert!(
            leaked.is_empty(),
            "faces::list as tenant A returned tenant B's faces: {leaked:?}"
        );
        assert!(
            !listed.is_empty(),
            "tenant A should see its own four faces — an empty result would pass the leak check \
             for the wrong reason"
        );
    });
}

/// Fetching a face BY ID while acting as the wrong tenant is the classic IDOR shape: the id is
/// known (it is in a URL), and the only thing standing between it and the data is a predicate.
#[test]
fn fetching_another_tenants_face_by_id_returns_nothing() {
    with_fixture(|db, _pool, _rt, f| {
        for id in &f.b_face_ids {
            assert!(
                faces::get(db, f.a, id).is_none(),
                "faces::get let tenant A read tenant B's face {id}"
            );
        }
        // And the same call for a face A owns must work, or the check above proves only that the
        // function is broken.
        let own = faces::list(db, f.a, 1);
        let own_id = &own.first().expect("tenant A has faces").id;
        assert!(
            faces::get(db, f.a, own_id).is_some(),
            "faces::get could not read tenant A's OWN face — the negative result above is not \
             evidence of isolation"
        );
    });
}

/// Recognition is a kNN over the union gallery. The fixtures put both tenants' vectors in the same
/// narrow region precisely so that distance alone cannot be what keeps B out.
#[test]
fn recognition_never_matches_against_another_tenants_gallery() {
    with_fixture(|db, _pool, _rt, f| {
        let probe = vector(101); // near B's seeds
        let matches = faces::recognize(db, f.a, &probe, 25);
        let leaked: Vec<&str> = matches
            .iter()
            .map(|m| m.reference.as_str())
            .filter(|r| f.b_face_ids.iter().any(|id| id == r))
            .collect();
        assert!(
            leaked.is_empty(),
            "faces::recognize as tenant A matched tenant B's stored faces: {leaked:?}"
        );
    });
}

/// `persons`' primary key is (tenant_id, id), so both tenants hold a person with the SAME id. A
/// query filtering on `id` alone returns the wrong customer's person — and would look correct.
#[test]
fn loading_people_never_returns_another_tenants_person() {
    with_fixture(|db, _pool, _rt, f| {
        let people = persons::repo::load(db, f.a);
        let names: Vec<&str> = people.iter().map(|p| p.name.as_str()).collect();
        assert!(
            !names.contains(&"Bob of B"),
            "persons::load as tenant A returned tenant B's person (saw {names:?}) — note both \
             tenants store the id {:?}",
            f.b_person
        );
        assert!(
            names.contains(&"Alice of A"),
            "tenant A should see its own person; saw {names:?}"
        );
    });
}

/// A tenant that owns nothing must see nothing — not everything. This is the shape a missing
/// predicate produces most visibly: a brand-new workspace showing another customer's data.
#[test]
fn a_tenant_with_no_data_sees_no_data() {
    with_fixture(|db, _pool, _rt, _f| {
        let empty = uuid::Uuid::new_v4();
        assert!(
            faces::list(db, empty, 500).is_empty(),
            "a tenant with no rows of its own saw faces belonging to someone else"
        );
        assert!(
            persons::repo::load(db, empty).is_empty(),
            "a tenant with no rows of its own saw people belonging to someone else"
        );
    });
}

/// Records what the database would enforce on its own today — currently nothing. This is the
/// baseline #160 Phase 2 has to move: it asserts the CURRENT state so the change is visible as a
/// diff rather than as a claim.
#[test]
fn baseline_the_database_enforces_no_isolation_of_its_own() {
    with_fixture(|_db, pool, rt, _f| {
        let (superuser, bypassrls): (bool, bool) = rt.block_on(async {
            let row = sqlx::query(
                "SELECT rolsuper, rolbypassrls FROM pg_roles WHERE rolname = current_user",
            )
            .fetch_one(pool)
            .await
            .expect("role attributes");
            (row.get(0), row.get(1))
        });
        let rls_tables: i64 = rt.block_on(async {
            sqlx::query_scalar(
                "SELECT count(*) FROM pg_tables WHERE schemaname = 'public' AND rowsecurity",
            )
            .fetch_one(pool)
            .await
            .unwrap_or(0)
        });
        println!(
            "baseline — connecting role: superuser={superuser} bypassrls={bypassrls}; \
             tables with RLS enabled: {rls_tables}"
        );
        // Deliberately NOT asserting these are false/zero. This test documents the starting point;
        // when Phase 2 lands, it is the assertion that has to be inverted, and the inversion is
        // how we prove the database is doing the work rather than the application.
        assert!(
            superuser || bypassrls || rls_tables == 0,
            "the database now enforces isolation — invert this test (see docs/plans/tenant-isolation-rls.md)"
        );
    });
}
