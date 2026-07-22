-- Per-tenant Tuya Cloud credentials ("bring your own Tuya project"): each tenant creates their own
-- Tuya IoT project, links their own Smart Life account (QR, on their own console — self-consent),
-- and pastes their Access ID / Access Secret here. Resolved tenant-first, falling back to the
-- operator-level env creds (TUYA_ACCESS_ID/KEY) for the Default/operator tenant. Same shape as the
-- Telegram per-tenant config. Credentials are a secret — write-only to the UI.
CREATE TABLE IF NOT EXISTS tuya_config (
  tenant_id  uuid PRIMARY KEY,
  access_id  text NOT NULL,
  access_key text NOT NULL,
  endpoint   text NOT NULL,         -- data-center base URL, e.g. https://openapi.tuyaus.com
  updated_at text NOT NULL          -- RFC3339 (sqlx has no timestamptz here)
);
