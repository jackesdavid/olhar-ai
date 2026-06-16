-- Phase 5b: edge appliances ("boxes") bound to a tenant. The edge presents its box
-- token to /config/cameras; control resolves token → tenant and returns ONLY that
-- tenant's cameras (config-light edge, no DB on the box). The edge reports its tailnet
-- host (magicdns_host) so the gateway can resolve a tenant's live upstream. The token
-- is stored hashed (sha256) — the plaintext is shown once at creation.
CREATE TABLE IF NOT EXISTS tenant_boxes (
    id            uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id     uuid NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    name          text NOT NULL,
    token_hash    text NOT NULL UNIQUE,
    magicdns_host text,
    last_seen     timestamptz,
    created_at    timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS tenant_boxes_tenant_idx ON tenant_boxes(tenant_id);
