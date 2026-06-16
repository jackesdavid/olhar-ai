-- Phase 5b+: edge reports its version, and pushes ONVIF-discovered cameras up so the
-- cloud UI can surface them (the edge dials out — no inbound needed).

-- The running edge build, reported on each /config/cameras pull (for "update available").
ALTER TABLE tenant_boxes ADD COLUMN IF NOT EXISTS version text;

-- Cameras the edge found on its LAN (WS-Discovery), per tenant. Ephemeral-ish: the edge
-- refreshes this set; the admin promotes an entry into `devices` (with credentials).
CREATE TABLE IF NOT EXISTS discovered_cameras (
    tenant_id    uuid NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    ip           text NOT NULL,
    name         text NOT NULL DEFAULT '',
    xaddr        text NOT NULL DEFAULT '',
    manufacturer text,
    model        text,
    onvif_id     text,
    last_seen    timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (tenant_id, ip)
);
