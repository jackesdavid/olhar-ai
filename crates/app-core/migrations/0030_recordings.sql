-- Per-box ring buffer + recording config (pre_roll_secs, max_storage_mb, codec).
-- Stored as JSONB so the schema can evolve without migrations per field.
ALTER TABLE tenant_boxes
    ADD COLUMN recording_config JSONB NOT NULL DEFAULT '{"pre_roll_secs":5,"max_storage_mb":1000,"codec":"libx264"}';

-- Clip metadata pushed by the edge when a recording is finalised.
-- Timestamps are RFC3339 TEXT (sqlx built without chrono on sentinel-flow).
CREATE TABLE recordings (
    id            BIGSERIAL PRIMARY KEY,
    tenant_id     UUID    NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    camera_ip     TEXT    NOT NULL,
    clip_id       TEXT    NOT NULL,   -- edge-local slug (unique per edge)
    duration_secs INTEGER NOT NULL DEFAULT 0,
    size_bytes    BIGINT  NOT NULL DEFAULT 0,
    started_at    TEXT    NOT NULL,   -- RFC3339
    created_at    TEXT    NOT NULL
);

CREATE INDEX recordings_tenant_camera ON recordings(tenant_id, camera_ip, started_at DESC);
