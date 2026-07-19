-- Faces labeling view (#119). Two additive changes on the `faces` store (#117):
--
-- 1. `ignored` — a SOFT dismiss for junk crops (a bad angle, a non-face the detector caught). Kept
--    recoverable (the crop + embedding stay) rather than a hard delete, so a mis-click doesn't lose
--    data. Ignored faces drop out of the triage queue + clustering.
-- 2. A partial index on the UNKNOWN, non-ignored set — exactly what the cluster/triage endpoints scan
--    on every call (`WHERE tenant_id = $1 AND person_id IS NULL AND NOT ignored`), so grouping stays
--    fast as the unknown pile grows.
ALTER TABLE faces ADD COLUMN IF NOT EXISTS ignored boolean NOT NULL DEFAULT false;

CREATE INDEX IF NOT EXISTS faces_tenant_unknown
    ON faces (tenant_id) WHERE person_id IS NULL AND NOT ignored;
