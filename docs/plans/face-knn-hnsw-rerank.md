# pgvector kNN Scale — HNSW Index + Retrieve-then-Rerank — Implementation Plan

**Board card:** pgvector face kNN — HNSW index + retrieve-then-rerank (ToDo)
**Goal:** Turn the brute-force cosine scans into indexed ANN retrieval + exact re-rank, so recognition
and dedup stay fast and exact-quality as per-tenant galleries grow. Includes the union-query
restructure that makes the index actually kick in.

## Why
There is **no vector index anywhere** — `person_embeddings` and `faces` only have btree indexes on
`person_id`/`tenant_id`. So `recognize` (`faces.rs:107`) and `nearest_stored_distance` (`faces.rs:143`)
are full sequential scans with `<=>`. Exact and fine at ~128 faces, but O(gallery) per event and a
per-tenant time bomb. The current `UNION ALL … ORDER BY distance` (`faces.rs:113`) also can't use an
ANN index — the index-ordered scan must be per-table.

## Phases
1. **Add HNSW indexes** (migration, next number after 0063):
   ```sql
   CREATE INDEX faces_embedding_hnsw ON faces
     USING hnsw (embedding vector_cosine_ops) WITH (m = 16, ef_construction = 64);
   CREATE INDEX person_embeddings_embedding_hnsw ON person_embeddings
     USING hnsw (embedding vector_cosine_ops) WITH (m = 16, ef_construction = 64);
   ```
   HNSW (not IVFFlat — no training step, better recall at this scale). **Validate on the live DB**
   before deploy ([[validate-migrations-live-db]]) — pgvector must be present (it is, migration 0001).
2. **Restructure the query.** Per-table HNSW-ordered top-k (each `ORDER BY embedding <=> $1 LIMIT n`),
   then merge the two candidate sets, then aggregate per `reference`. This is the enabler for
   [[face-recognition-per-person-scoring]]'s GROUP-BY scoring.
3. **Two-stage retrieve-then-rerank.** Pull ~50 candidates via the index, exact-`<=>`-rerank + apply
   per-person aggregation/margin in the merge step. Keeps exact top-k quality despite ANN being
   approximate.
4. **Tune `hnsw.ef_search`** per query (`SET LOCAL`) for the recall/latency trade; keep the existing
   `SET LOCAL statement_timeout` guard.
5. **Feed the clustering graph.** Expose a top-k-neighbours helper so [[face-clustering-chaining-fix]]
   can build its kNN graph from the index instead of all-pairs.

## Open decisions
1. `m` / `ef_construction` / candidate `n` — tune on real gallery sizes.
2. Whether index build needs a bumped `maintenance_work_mem` on the VPS Postgres
   (coordinate with [[postgres-performance-tuning]]).

## Out of scope (v1)
- Per-tenant partitioned indexes / sharding. Single index per table with the `tenant_id` filter.

## Related
[[face-recognition-per-person-scoring]] · [[face-clustering-chaining-fix]] · [[devices-postgres-migration]] · [[validate-migrations-live-db]]
