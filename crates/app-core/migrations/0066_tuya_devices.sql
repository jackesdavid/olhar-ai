-- Tuya smart-home devices linked to the operator's single Tuya Cloud project, scoped
-- per tenant (v1: the QR "Tuya App Account Authorization" links an app account; sync
-- pulls its devices here). Credentials themselves live only in env (operator-level),
-- never in this table — this is the credential-free device catalog the editor + nodes read.
CREATE TABLE IF NOT EXISTS tuya_devices (
  tenant_id      uuid NOT NULL,
  device_id      text NOT NULL,               -- Tuya device id (stable) — the selector nodes store
  name           text NOT NULL,               -- friendly name (display label only)
  category       text,                        -- Tuya category code (e.g. "kg" = switch)
  product_id     text,
  online         boolean NOT NULL DEFAULT false,
  functions_json text,                         -- JSON array of {code,type,values} — types for the editor
  tuya_uid       text,                         -- linked app-account uid (hook for multi-tenant pairing)
  updated_at     text NOT NULL,               -- RFC3339 (sqlx has no timestamptz here)
  PRIMARY KEY (tenant_id, device_id)
);
