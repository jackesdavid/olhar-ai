-- Edge resource-usage snapshot, reported on the heartbeat alongside health (the
-- `X-Edge-Metrics` JSON header on /config/cameras) so the Boxes page can show whether an
-- edge is running optimally. Latest-value snapshot (no time-series), mirroring the 0018
-- health columns. Percentages are 0..100; gpu_* are NULL on boxes without an NVIDIA GPU.
ALTER TABLE tenant_boxes ADD COLUMN IF NOT EXISTS cpu_pct     integer;
ALTER TABLE tenant_boxes ADD COLUMN IF NOT EXISTS mem_pct     integer;
ALTER TABLE tenant_boxes ADD COLUMN IF NOT EXISTS disk_pct    integer;
ALTER TABLE tenant_boxes ADD COLUMN IF NOT EXISTS gpu_pct     integer;
ALTER TABLE tenant_boxes ADD COLUMN IF NOT EXISTS gpu_mem_pct integer;
ALTER TABLE tenant_boxes ADD COLUMN IF NOT EXISTS metrics_at  timestamptz;
