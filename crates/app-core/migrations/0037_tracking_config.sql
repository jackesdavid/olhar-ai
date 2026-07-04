-- Per-box object-tracking config (card: edge tracking, P3+): `enabled`/`detect_ms` are box-global;
-- `trackers` is the job list — one `{camera, class, lines[]}` entry per distinct camera+class
-- pair some enabled graph's OnEvent("object_entered"/"object_exited"/"line_crossed") node cares
-- about (extracted from those nodes' property-panel config on graph save; two nodes naming the
-- same camera+class share one job, their lines merged). Stored as JSONB so the shape can evolve
-- without migrations per field, mirroring recording_config (0030_recordings.sql).
ALTER TABLE tenant_boxes
    ADD COLUMN tracking_config JSONB NOT NULL DEFAULT '{"enabled":false,"detect_ms":400,"trackers":[]}';
