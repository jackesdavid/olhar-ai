-- Phase 3: multiple face embeddings per person, each with its own id and approval.
-- Supersedes the single `persons.embedding` / `persons.consent` columns (Phase 2):
-- recognition becomes one kNN over all approved rows, and "is this person enrolled?"
-- is derived as "has ≥1 approved embedding".
CREATE TABLE IF NOT EXISTS person_embeddings (
    id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    person_id   text NOT NULL REFERENCES persons(id) ON DELETE CASCADE,
    embedding   vector(512) NOT NULL,
    approved    boolean NOT NULL DEFAULT false,
    approved_at timestamptz,
    source      text NOT NULL DEFAULT '', -- 'camera' | 'upload'
    created_at  timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS person_embeddings_person_id_idx ON person_embeddings (person_id);

-- Retire the per-person single-embedding design from Phase 2.
ALTER TABLE persons DROP COLUMN IF EXISTS embedding;
ALTER TABLE persons DROP COLUMN IF EXISTS consent;
