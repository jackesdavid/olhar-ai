-- Per-box HMAC-SHA256 request-signing keys (edge → control).
--
-- The appliance authenticates with a static bearer that never expires and is rotated only at
-- re-enrollment. It proves who is calling and nothing more: no request is bound to its body, and a
-- captured request replays forever — including the pushes that enqueue straight into the workflow
-- event queue. These keys are what a signature is computed with.
--
-- UNLIKE every other credential in this schema, `secret` is stored **plaintext**. A hash cannot
-- verify a MAC — the verifier needs the key itself. That puts it in the same category as
-- `tuya_config.access_key` and `telegram_config.bot_token`: replayable secrets, write-only to any
-- UI, with the database as the trust boundary.
--
-- Its own table rather than a column on `tenant_boxes`, and that is structural rather than stylistic:
-- `store::BoxRow` derives Serialize and is returned wholesale to the Boxes page, so a secret column
-- there leaks the day someone adds the field to the struct.
CREATE TABLE IF NOT EXISTS box_signing_keys (
    id         uuid PRIMARY KEY DEFAULT gen_random_uuid(),  -- the key_id carried on the wire
    box_id     uuid NOT NULL REFERENCES tenant_boxes(id) ON DELETE CASCADE,
    -- Denormalized so verification resolves box + tenant in one lookup, on every signed request.
    tenant_id  uuid NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    secret     text NOT NULL,                                -- 32 bytes OsRng, base64url-nopad
    algo       text NOT NULL DEFAULT 'hmac-sha256',
    created_at text NOT NULL,                                -- RFC3339 (house rule)
    -- Set when a key is superseded: both the old and new key verify until this passes, so a box
    -- that rotates mid-flight doesn't 401 its own in-flight requests.
    expires_at text,
    revoked_at text
);

-- Verification looks up live keys for a box on every signed request; rotation means a box can
-- legitimately have more than one.
CREATE INDEX IF NOT EXISTS box_signing_keys_box_idx
    ON box_signing_keys (box_id) WHERE revoked_at IS NULL;

-- Per-box rollout state. A DB-recorded, per-box decision rather than a global env var, so
-- enforcement can advance one box at a time and a straggler can be left behind instead of broken.
-- 'off'      — unsigned requests are accepted (an invalid signature is ALWAYS refused, in any mode)
-- 'required' — a valid signature is mandatory
ALTER TABLE tenant_boxes ADD COLUMN IF NOT EXISTS signing_mode text NOT NULL DEFAULT 'off';

-- Fleet-readiness telemetry. `last_unsigned_at` is the one that matters: `last_signed_at` only
-- proves *some* code path signs, while a stale `last_unsigned_at` proves every path does — which is
-- the difference between flipping a box to 'required' safely and 401ing its rarely-fired endpoints
-- (clip-confirm, disk-alert) days later.
ALTER TABLE tenant_boxes ADD COLUMN IF NOT EXISTS last_signed_at text;
ALTER TABLE tenant_boxes ADD COLUMN IF NOT EXISTS last_unsigned_at text;
