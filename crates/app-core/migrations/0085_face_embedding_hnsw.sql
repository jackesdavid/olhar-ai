-- #127: HNSW indexes on both face galleries, so recognition stops being a full sequential scan.
--
-- Until now there was NO vector index anywhere: `faces` (0061) and `person_embeddings` (0003/0011)
-- carried only btrees on tenant_id/person_id, so every recognition computed `<=>` against every row
-- of both tables. Measured on a 5 200-vector gallery: 135 ms mean / 331 ms p95 per recognition, and
-- the face pipeline fires on every motion-gated frame.
--
-- HNSW rather than IVFFlat: no training step (IVFFlat needs representative data present at build
-- time, which is false for a tenant that starts empty and grows), and better recall at this scale.
--
-- `vector_cosine_ops` matches the `<=>` operator the queries use. It is the correct opclass here
-- because embeddings are L2-normalized at store time (#125, `faces::l2_normalize`) — cosine and
-- inner-product orderings coincide only under that guarantee.
--
-- THE INDEX ALONE DOES NOTHING. pgvector serves an index scan only for
-- `ORDER BY <vector column> <operator> <constant> LIMIT n` evaluated directly against one table.
-- The queries in `faces.rs` violated that twice over (distance computed inside a UNION ALL; the
-- result cast `::float4`, which stops the sort expression matching the index's ordering operator).
-- Both are rewritten in the same commit as this migration — `crates/app-core/tests/face_knn_bench.rs`
-- asserts on the EXPLAIN output that an index scan is genuinely chosen, because a healthy-looking
-- index next to a sequential scan is the exact failure this card exists to prevent.
--
-- Plain CREATE INDEX, not CONCURRENTLY: sqlx wraps each migration in a transaction and
-- CREATE INDEX CONCURRENTLY cannot run inside one. Both tables are small today, so the brief write
-- lock is acceptable. When they are not, this needs a `-- no-transaction` migration — a form no
-- migration in this repo uses yet, so treat it as unproven rather than routine.
CREATE INDEX IF NOT EXISTS faces_embedding_hnsw
    ON faces USING hnsw (embedding vector_cosine_ops)
    WITH (m = 16, ef_construction = 64);

CREATE INDEX IF NOT EXISTS person_embeddings_embedding_hnsw
    ON person_embeddings USING hnsw (embedding vector_cosine_ops)
    WITH (m = 16, ef_construction = 64);
