-- Mobile push devices (card #139, Phase 1) — a Recipient delivery channel that runs PARALLEL to
-- Telegram (`notification_targets`), deliberately NOT a column bolted onto it: Telegram delivery
-- stays byte-for-byte unchanged, and a tenant can have both.
--
-- `device_token` is the Expo push token (Phase 1 uses Expo's push service as the single endpoint
-- for both APNs and FCM — see the plan's open decision #1). Timestamps are RFC3339 `text`, per the
-- project rule (sqlx is built without the chrono/time feature, so it can't decode timestamptz).
CREATE TABLE IF NOT EXISTS push_devices (
    id           uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id    uuid NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    -- Which operator's phone this is. NULL-able so a device can outlive a user row.
    user_id      uuid REFERENCES users(id) ON DELETE SET NULL,
    device_token text NOT NULL,
    platform     text NOT NULL DEFAULT 'unknown', -- ios | android | unknown
    -- Human label shown in the Recipients screen ("Jackes' iPhone").
    label        text NOT NULL DEFAULT '',
    enabled      boolean NOT NULL DEFAULT true,
    created_at   text NOT NULL,
    last_seen    text
);

-- Re-registering the same token (app reinstall / token refresh) updates in place instead of
-- fanning out duplicate pushes to one phone.
CREATE UNIQUE INDEX IF NOT EXISTS push_devices_tenant_token_uniq
    ON push_devices (tenant_id, device_token);

-- The dispatch-time lookup: every enabled device for a tenant.
CREATE INDEX IF NOT EXISTS push_devices_tenant_enabled_idx
    ON push_devices (tenant_id, enabled);
