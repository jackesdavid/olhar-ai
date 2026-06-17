-- Presence: categorized detections (person / known person / car / cat / dog /
-- unknown) written by the RecordPresence workflow node when movement/recognition
-- fires. Persisted so the map can replay recent presence; surfaced live as a pulse.
-- Timestamps are RFC3339 text (sqlx is built without chrono/time → no timestamptz).
CREATE TABLE IF NOT EXISTS presence (
    tenant_id  uuid   NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    id         bigint NOT NULL,                       -- per-tenant monotonic
    category   text   NOT NULL DEFAULT 'unknown',
    label      text,                                  -- optional caption (e.g. person name)
    room_id    bigint,                                -- map room, if resolved
    room_name  text,                                  -- name at record time
    camera     text,                                  -- detecting camera/ip, if known
    source     text   NOT NULL DEFAULT 'workflow',
    created_at text   NOT NULL,                       -- RFC3339
    PRIMARY KEY (tenant_id, id)
);

CREATE INDEX IF NOT EXISTS presence_tenant_idx ON presence(tenant_id, id);
