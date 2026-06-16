-- Edge box logs streamed up from each box for in-UI observability. The edge keeps its
-- own bounded ring + stdout; this is the cloud-side persisted tail so logs survive a
-- control restart and are viewable per box. Bounded by a prune on ingest (keep newest
-- per box) — see store::prune_box_logs.
CREATE TABLE IF NOT EXISTS box_logs (
    id         bigserial PRIMARY KEY,
    tenant_id  uuid NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    box_id     uuid REFERENCES tenant_boxes(id) ON DELETE CASCADE,
    ts         text NOT NULL,                  -- RFC3339 from the edge (matches alerts.created_at)
    level      text NOT NULL,
    target     text NOT NULL DEFAULT '',
    message    text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS box_logs_box_idx ON box_logs (tenant_id, box_id, id DESC);
