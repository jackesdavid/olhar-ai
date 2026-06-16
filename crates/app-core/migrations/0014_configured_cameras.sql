-- A non-sensitive marker that a camera has had its credentials set (relayed to the
-- edge). The credentials themselves are NEVER stored in the cloud — only this flag, so
-- the UI can show "Configured" instead of a misleading "None".
CREATE TABLE IF NOT EXISTS configured_cameras (
    tenant_id uuid NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    ip        text NOT NULL,
    PRIMARY KEY (tenant_id, ip)
);
