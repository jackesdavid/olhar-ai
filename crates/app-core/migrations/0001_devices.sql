-- First module → Postgres: devices.
-- pgvector is enabled here so future modules can add embedding/snapshot vector
-- columns without a separate bootstrap migration.
CREATE EXTENSION IF NOT EXISTS vector;

CREATE TABLE IF NOT EXISTS devices (
    ip           text PRIMARY KEY,          -- upserts key on ip today
    id           text NOT NULL DEFAULT '',  -- stable ONVIF urn (empty on legacy rows)
    xaddr        text NOT NULL DEFAULT '',  -- ONVIF service address
    name         text NOT NULL DEFAULT '',
    manufacturer text,
    model        text,
    username     text,
    password     text,
    rtsp_url     text                       -- set once credentials validate
);
