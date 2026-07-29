-- WebAuthn / FIDO2 passkey credentials (board card #147, security plan §2).
--
-- Each row is one registered authenticator — a roaming key OR a platform authenticator
-- (Touch ID / Face ID / Windows Hello / an Android passkey); the register/authenticate options
-- leave `authenticatorAttachment` unset so both flow through the same table. `passkey` is the
-- serialized webauthn-rs `Passkey` (it already holds the COSE public key, the signature counter
-- and the transports); `credential_id` is lifted out as its own column for O(1) lookup + a
-- uniqueness guard so the same authenticator can't be registered twice.
--
-- Timestamps are RFC3339 `text` (sqlx is built without the timestamptz decoder), matching the rest
-- of the schema — produced by `app_core::timeutil::now_rfc3339()`.
CREATE TABLE IF NOT EXISTS user_credentials (
    id            uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id       uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    credential_id bytea NOT NULL,
    passkey       jsonb NOT NULL,
    nickname      text,
    created_at    text NOT NULL,
    last_used_at  text
);
CREATE UNIQUE INDEX IF NOT EXISTS user_credentials_cred_id_idx ON user_credentials (credential_id);
CREATE INDEX IF NOT EXISTS user_credentials_user_idx ON user_credentials (user_id);

-- Per-user rollout state (E0 → E2). Nothing enforces these in E0; they exist so the E1
-- enroll-after-login nudge and the eventual E2 enforcement are decided PER USER, never a single
-- global flag flip that would lock the whole fleet (you included) out at once. `grace_until` is
-- RFC3339 text like every other timestamp here.
ALTER TABLE users ADD COLUMN IF NOT EXISTS webauthn_required boolean NOT NULL DEFAULT false;
ALTER TABLE users ADD COLUMN IF NOT EXISTS grace_until text;
