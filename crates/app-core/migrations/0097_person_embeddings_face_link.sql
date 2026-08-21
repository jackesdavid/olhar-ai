-- Card #200: delete means delete.
--
-- Labeling a face copies its embedding into `person_embeddings` (faces::label_faces). Deleting the
-- face then removed the row and its MinIO objects but LEFT the embedding behind, so recognition
-- kept matching a face nobody could see any more — 104 embeddings against 3 surviving faces in
-- production. That is a correctness problem and a privacy one: "delete this face" has to mean it.
--
-- Record which face produced an embedding, and let the database enforce the cascade rather than a
-- second statement someone can forget. Nullable + additive: existing rows keep NULL (they predate
-- the link, and their faces are already gone), so this is safe to apply to a live table.
ALTER TABLE person_embeddings
  ADD COLUMN IF NOT EXISTS face_id uuid REFERENCES faces(id) ON DELETE CASCADE;

CREATE INDEX IF NOT EXISTS person_embeddings_face_id_idx
  ON person_embeddings (face_id) WHERE face_id IS NOT NULL;
