-- Face recognition pipeline (#117) — the tenant FACE STORE: every face the pipeline decides to keep,
-- labeled (person_id → a real person) or UNKNOWN (person_id NULL). Distinct from `person_embeddings`
-- (0003), which only holds *enrolled* faces of known people; this table also holds unknowns so they
-- can cluster and match each other before anyone labels them.
--
-- Recognition kNN is the UNION of `person_embeddings` (enrolled) + `faces` (stored), each match
-- tagged by source. Same pgvector `vector(512)` ArcFace shape as `person_embeddings` (a fixed size is
-- required to compare across the two galleries). The face image lives in MinIO (`image_key`); the row
-- keeps only the vector + metadata. Timestamps are RFC3339 text (sqlx has no chrono/time — CLAUDE.md).
CREATE TABLE IF NOT EXISTS faces (
    id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id   uuid NOT NULL,
    -- The label. NULL = unknown. An app-level reference to persons(id) — NOT a hard FK, because
    -- `persons`' PK is composite (tenant_id, id) since migration 0011, so `id` alone has no unique
    -- constraint to reference. Deleting a person leaves the stored capture with a dangling label
    -- (the app treats an unresolved person_id as unknown; a later cleanup can null it).
    person_id   text,
    embedding   vector(512) NOT NULL,
    image_key   text,              -- MinIO object key of the face image (NULL = embedding-only)
    camera      text,
    bbox        jsonb,             -- the face bbox on the source frame (xyxy)
    confidence  real NOT NULL DEFAULT 0,
    face_ratio  real NOT NULL DEFAULT 0, -- face area ÷ frame area
    source      text NOT NULL DEFAULT '', -- 'edge' | 'snapshot' | 'upload' | …
    created_at  text NOT NULL
);

CREATE INDEX IF NOT EXISTS faces_tenant_idx ON faces (tenant_id);
CREATE INDEX IF NOT EXISTS faces_tenant_person_idx ON faces (tenant_id, person_id);
