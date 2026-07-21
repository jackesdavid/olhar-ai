-- Tuya v2: per-function friendly labels + self-service OAuth account linking.

-- (1) Friendly labels for a device's data points (code → label), e.g. switch_1 → "Luz da entrada".
-- Kept OUT of the sync writer's ON CONFLICT DO UPDATE SET (tuya::repo::upsert_many) so a "Refresh
-- devices" never clobbers user labels. Shape: JSON object { "<code>": "<label>" }.
ALTER TABLE tuya_devices ADD COLUMN IF NOT EXISTS function_labels_json text;

-- (2) Per-tenant link to a Tuya/Smart Life account (OAuth 2.0). A tenant may link one or more app
-- accounts; each linked account is identified by its Tuya uid. `sync` filters
-- associated-users/devices by the tenant's linked uid(s). Timestamps are RFC3339 text.
CREATE TABLE IF NOT EXISTS tuya_account_links (
  tenant_id  uuid NOT NULL,
  tuya_uid   text NOT NULL,
  linked_at  text NOT NULL,
  PRIMARY KEY (tenant_id, tuya_uid)
);
