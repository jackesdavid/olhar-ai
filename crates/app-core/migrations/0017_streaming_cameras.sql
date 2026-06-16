-- Which cameras the edge currently has a RESOLVED, working stream for (rtsp_url resolved
-- via ONVIF → creds valid + camera reachable). The edge pushes its full current set; the
-- Devices page uses it to show a precise "online & correct" status dot (vs. just
-- "configured + visible"). Mirrors configured_cameras (replace-all per push).
CREATE TABLE IF NOT EXISTS streaming_cameras (
    tenant_id uuid NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    ip        text NOT NULL,
    PRIMARY KEY (tenant_id, ip)
);
