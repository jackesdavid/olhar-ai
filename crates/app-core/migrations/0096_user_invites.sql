-- User invites (#181) — how a person joins a tenant without anyone reading a password aloud.
--
-- The admin creates one of these; the invitee redeems it from the web link or by scanning the QR
-- on their phone. It replaces the current shape, where `invite_user` takes an ADMIN-CHOSEN password
-- and sends nothing.
CREATE TABLE IF NOT EXISTS user_invites (
    id            uuid PRIMARY KEY,
    tenant_id     uuid NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,

    -- The token is stored HASHED, never in the clear. It is a bearer credential that creates an
    -- account: anyone holding the row would otherwise be able to join the tenant. Same rule the
    -- Device Gate token follows (migration 0094).
    token_sha256  text NOT NULL UNIQUE,

    -- A short, human-typeable form of the same invite, for a phone whose camera is refused. Also
    -- unique, and looked up the same way — it is the token, not a second credential.
    code          text NOT NULL UNIQUE,

    -- Optional: a QR handed over in person needs no address, and requiring one would invent a fake.
    email         text,

    -- Roles to grant on accept, as a JSON array of role ids. Chosen by the admin at creation —
    -- anything else means every invited person needs a second admin action to become useful.
    role_ids_json text NOT NULL DEFAULT '[]',

    -- RFC3339 text, not timestamptz: sqlx is built without chrono/time and cannot decode
    -- timestamptz (see CLAUDE.md).
    created_at    text NOT NULL,
    expires_at    text NOT NULL,

    -- Set exactly once, by the conditional UPDATE that redeems the invite. NULL = still open.
    accepted_at   text,
    accepted_user uuid REFERENCES users(id) ON DELETE SET NULL,

    created_by    uuid REFERENCES users(id) ON DELETE SET NULL
);

-- The two lookup paths: redeeming by token (hashed) and by typed code. Both are unauthenticated
-- and enumerable, so they must be index hits rather than scans.
CREATE INDEX IF NOT EXISTS idx_user_invites_tenant ON user_invites (tenant_id);
CREATE INDEX IF NOT EXISTS idx_user_invites_open ON user_invites (expires_at) WHERE accepted_at IS NULL;
