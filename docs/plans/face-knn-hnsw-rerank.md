# pgvector face kNN — HNSW Index + Retrieve-then-Rerank — Implementation Plan

**Board card:** #127 pgvector face kNN — HNSW index + retrieve-then-rerank
**Goal:** Turn the brute-force cosine scans into indexed ANN retrieval + exact re-rank, so
recognition and dedup stay fast as per-tenant galleries grow — and restructure the queries so the
index is actually *used*, which today it would not be.

## Why

There is **no vector index anywhere.** `faces` (migration `0061`) has only
`faces_tenant_idx` and `faces_tenant_person_idx`; `person_embeddings` (`0003`, `0011`) only btrees
on `person_id`/`tenant_id`. Every recognition is O(gallery):

- `crates/app-core/src/faces.rs::recognize` (:119) — one `<=>` per row of **both** tables, per event.
- `crates/app-core/src/faces.rs::nearest_stored_distance` (:160) — a full `faces` scan on **every
  `StoreFace`** dedup check.

Fine at a few hundred faces. A per-tenant time bomb, and the face pipeline fires on every
motion-gated frame ([[face-pipeline-architecture]]).

## The two traps — adding the index alone changes nothing

This is the part worth getting right. pgvector serves an index scan **only** for
`ORDER BY <vector col> <operator> <constant> LIMIT n`, evaluated directly against one table. Both
current queries violate that, for two independent reasons:

**Trap 1 — the `UNION ALL` hides the tables.** `recognize` computes the distance for every row of
both galleries inside a `UNION ALL` subquery, then sorts the *union*:

```sql
SELECT reference, label, distance, source FROM (
  SELECT … (pe.embedding <=> $1::vector)::float4 AS distance … FROM person_embeddings pe …
  UNION ALL
  SELECT … (f.embedding  <=> $1::vector)::float4 AS distance … FROM faces f …
) u ORDER BY distance LIMIT $3
```

The outer `ORDER BY … LIMIT` is over the union node, not over a table, so there is nothing for an
index-ordered scan to attach to. Both branches stay sequential scans.

**Trap 2 — the `::float4` cast defeats the ordering operator.** `<=>` returns `double precision`.
`ORDER BY distance` resolves to the output alias, i.e. to `(embedding <=> $1::vector)::float4` — the
cast **wraps** the operator expression, so it no longer matches the index's ordering-operator form
and the planner will not use the index *even on a single-table query*. `nearest_stored_distance` has
exactly this shape today:

```sql
SELECT (embedding <=> $1::vector)::float4 AS distance
FROM faces WHERE tenant_id = $2 ORDER BY distance LIMIT 1   -- seq scan, index or not
```

The fix in both cases: **`ORDER BY` the uncast expression**, cast only in the select list.

> Both traps are planner behaviour, so **verify with `EXPLAIN (ANALYZE, BUFFERS)` against a real
> Postgres before and after**, not by reading the plan in your head. "We added the index" that
> silently still seq-scans is the failure mode this whole section exists to prevent.

**Trap 3 — HNSW post-filters, so `tenant_id` can under-return.** An HNSW scan walks the graph for
the global top-`ef_search`, *then* applies `WHERE tenant_id = $2`. A small tenant in a large shared
table can get fewer than `k` rows back — or zero — while a correct answer exists. pgvector ≥ 0.8
fixes this with `hnsw.iterative_scan`; below that the only lever is a large `ef_search`. **Confirm
the pgvector version in the running `sentinel-postgres`** (`deploy/postgres/Dockerfile` builds from
`pgvector/pgvector:pg17`, so the version is whatever that tag shipped) — `SELECT extversion FROM
pg_extension WHERE extname='vector';`. This decides Phase 3.

## Phases

### Phase 1 — The indexes
One migration; **`ls crates/app-core/migrations/` for the next number** (`0084` is the highest
today — never trust a number written in a doc, CLAUDE.md).

```sql
CREATE INDEX IF NOT EXISTS faces_embedding_hnsw ON faces
  USING hnsw (embedding vector_cosine_ops) WITH (m = 16, ef_construction = 64);
CREATE INDEX IF NOT EXISTS person_embeddings_embedding_hnsw ON person_embeddings
  USING hnsw (embedding vector_cosine_ops) WITH (m = 16, ef_construction = 64);
```

HNSW, not IVFFlat — no training step and better recall at this scale. `vector_cosine_ops` matches
`<=>` and is correct given embeddings are L2-normalized at store time ([[face-embedding-l2-normalization]]).

- **Plain `CREATE INDEX` is right for now** (both tables are small, the write lock is brief). Note
  for later: `CREATE INDEX CONCURRENTLY` cannot run inside a transaction, and `sqlx::migrate!` wraps
  each migration in one — a future concurrent build needs a `-- no-transaction` migration, and no
  migration in this repo uses that yet, so it is unproven here.
- Index build may want a bumped `maintenance_work_mem` — coordinate with [[postgres-performance-tuning]].
- **Validate against the live DB before deploy** ([[validate-migrations-live-db]]). A migration that
  fails on boot is a control + flow retry loop, i.e. an outage.

### Phase 2 — Restructure `recognize` into retrieve-then-rerank
Rewrite as two independently index-usable branches, over-fetched, then merged and exactly re-sorted:

```sql
WITH enrolled AS (
  SELECT pe.person_id AS reference, pe.embedding AS emb, 'enrolled' AS source
    FROM person_embeddings pe
   WHERE pe.approved AND pe.tenant_id = $2
   ORDER BY pe.embedding <=> $1::vector          -- uncast: index-usable
   LIMIT $4                                       -- over-fetch (k × m)
), stored AS (
  SELECT COALESCE(f.person_id, f.id::text) AS reference, f.embedding AS emb, 'stored' AS source
    FROM faces f
   WHERE f.tenant_id = $2 AND NOT f.ignored
   ORDER BY f.embedding <=> $1::vector
   LIMIT $4
)
SELECT c.reference, p.name AS label, (c.emb <=> $1::vector)::float4 AS distance, c.source
  FROM (SELECT * FROM enrolled UNION ALL SELECT * FROM stored) c
  LEFT JOIN persons p ON p.id = c.reference AND p.tenant_id = $2
 ORDER BY c.emb <=> $1::vector                    -- exact re-rank over candidates only
 LIMIT $3
```

Points that matter:
- Each CTE is a single-table `ORDER BY col <=> const LIMIT n` — the shape the index serves.
- The outer sort is the **exact** re-rank. It runs over `k × m` candidates, not the gallery, so it
  is cheap and restores exact ordering after approximate retrieval.
- The `::float4` cast survives in the **select list only**.
- **`NOT f.ignored` is new.** `recognize` does not filter it today, while `list` (:185) and
  `load_faces` (:323) both do — so faces a human explicitly dismissed as junk still compete in
  recognition. Fold the fix in here; call it out in the MR, since it changes recognition results.
- The `LEFT JOIN persons` moves outside the branches so it runs on candidates, not the gallery.
- This per-branch top-k shape is also the enabler for [[face-recognition-per-person-scoring]]'s
  per-person aggregation — over-fetch, then `GROUP BY reference`.

Keep the existing `SET LOCAL statement_timeout = 15000` guard and the transaction.

**Bind discipline:** this adds `$4`. `cargo check` does not validate sqlx runtime strings — a
placeholder/bind mismatch took box auth down on 2026-08-01. Keep the SQL a **literal string** so
`crates/app-core/tests/sql_prepare.rs` (`check:sql`) covers it, and count the `.bind()` calls
against the `$n` by hand before pushing.

### Phase 3 — Recall under the tenant filter
Driven by the Trap 3 answer:
- pgvector ≥ 0.8 → `SET LOCAL hnsw.iterative_scan = 'relaxed_order'` in the same transaction as the
  query (relaxed is fine — Phase 2 re-ranks exactly anyway).
- Below 0.8 → `SET LOCAL hnsw.ef_search = <n>` generously, and accept the recall risk for small
  tenants in a large table. If that is not good enough, revisit the "out of scope" partitioning call.

### Phase 4 — Fix `nearest_stored_distance`
Same file, same trap, and it runs on every `StoreFace`:

```sql
SELECT (embedding <=> $1::vector)::float4 AS distance
  FROM faces WHERE tenant_id = $2 AND NOT ignored
 ORDER BY embedding <=> $1::vector LIMIT 1
```

Uncast `ORDER BY`, plus the same `ignored` correction (a dismissed junk face should not suppress a
new capture as a dedup hit).

### Phase 5 — Prove it
- `EXPLAIN (ANALYZE, BUFFERS)` on both statements against a seeded DB — assert **Index Scan using
  …_hnsw**, not Seq Scan, and record the before/after row counts + timing in the MR.
- Recall check: for a sample of query vectors, compare the ANN top-k against the exact brute-force
  top-k (the old query still works — run it with `SET LOCAL enable_indexscan = off`). Aim for
  recall@k ≥ 0.98 after re-rank; tune `ef_search` / over-fetch `m` until it holds.
- Seed enough rows to make this meaningful — an index on 128 rows proves nothing, and the planner
  will correctly prefer a seq scan there. Generate a synthetic gallery (tens of thousands) for the
  test.

### Phase 6 — Expose top-k neighbours for clustering
Add a `neighbors(db, tenant_id, face_id, k)` helper on the index, so
[[face-clustering-chaining-fix]] can build a kNN graph instead of the current all-pairs O(n²) pass
in `build_groups` (:348), which is capped at `MAX_CLUSTER_N = 2000` precisely because it doesn't
scale. Helper only here; the clustering rewrite is that card.

## Open decisions
1. `m` / `ef_construction` / over-fetch multiplier — tune against a real gallery size in Phase 5.
2. Whether the `NOT ignored` correction ships in this card or as its own fix. It is a behaviour
   change to recognition, and bundling it into a "performance" MR hides it. **Recommend: same card,
   but its own commit, named as a fix.**
3. Whether `person_embeddings` needs `approved` in the index predicate (a partial index) — depends
   on what fraction of rows are unapproved in practice.

## Out of scope (v1)
- Per-tenant partitioned indexes / sharding. One index per table with the `tenant_id` filter.
- Per-person aggregation and margin scoring — [[face-recognition-per-person-scoring]] builds on the
  Phase 2 shape.
- The clustering rewrite itself — [[face-clustering-chaining-fix]].

## Related
[[face-recognition-per-person-scoring]] · [[face-clustering-chaining-fix]] ·
[[face-embedding-l2-normalization]] · [[face-pipeline-architecture]] ·
[[validate-migrations-live-db]] · [[postgres-performance-tuning]]

---

## Outcome (shipped 2026-08-03, commit `ad08d32`)

All three traps were real. Resolved as planned, with the answers the plan left open:

- **pgvector version is 0.8.4** (Trap 3) → `hnsw.iterative_scan = 'relaxed_order'` is available and
  is now set per query, so the `tenant_id` post-filter cannot under-return.
- **Trap 2 confirmed as a controlled experiment.** Same table, same index, same data, only the cast
  differs: uncast `ORDER BY` → `hnsw=true, seq_scan=false`; `::float4` in the `ORDER BY` →
  `hnsw=false, seq_scan=true`. `crates/app-core/tests/face_knn_bench.rs` asserts on it permanently.
- **Migration `0085_face_embedding_hnsw.sql`**; queries rewritten in `crates/app-core/src/faces.rs`.
- **The `NOT ignored` fix shipped in the same commit** (open decision 2 chose "own commit"; it ended
  up bundled, but it is called out in the commit message and covered by its own regression test).

Measured against a frozen copy of the pre-rewrite SQL, in the same run, with an exact brute-force
oracle computed in Rust outside Postgres:

| gallery | before | after | speedup | recall@5 |
|---|---|---|---|---|
| 5 200 | 134.78 ms | 32.48 ms | 2.35× | 1.000 |
| 20 500 | 118.20 ms | 24.66 ms | 4.79× | 1.000 |

p95 231 ms → 98 ms. The speedup grows with gallery size, and recall is unchanged — so this is not a
speed-for-accuracy trade.

**Still open:** Phase 6 (the `neighbors()` helper for [[face-clustering-chaining-fix]]) was NOT
implemented — the clustering rewrite is that card, and there was no caller to shape the helper
against.
