-- The edge reports its platform (OS (arch), e.g. "macOS (aarch64)" / "Bazzite (x86_64)")
-- on the heartbeat (X-Edge-Platform); surfaced as a badge on the Boxes page.
ALTER TABLE tenant_boxes ADD COLUMN IF NOT EXISTS platform text;
