-- Persistent edge command flags so they survive control restarts.
-- Previously these were in-memory (HashSet in AppState) and lost on every deploy.
ALTER TABLE tenant_boxes
    ADD COLUMN IF NOT EXISTS pending_restart BOOLEAN NOT NULL DEFAULT FALSE,
    ADD COLUMN IF NOT EXISTS pending_update  BOOLEAN NOT NULL DEFAULT FALSE;
