-- Presence pulse lifetime (seconds) chosen on the RecordPresence node, persisted so the
-- map's replay-on-open honors it too (not just the live SSE event). Additive; default 8.
ALTER TABLE presence ADD COLUMN IF NOT EXISTS duration_secs integer NOT NULL DEFAULT 8;
