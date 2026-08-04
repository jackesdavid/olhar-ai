-- Card #166 — self-service password reset.
--
-- Until now the ONLY reset was an admin setting another user's password, so a self-serve customer
-- who forgot theirs had no way back into an account they were already paying for.
--
-- Only a SHA-256 HASH of the token is stored. A dump of this table must not be enough to take an
-- account: the raw token exists in exactly two places — the URL in the user's inbox, and memory
-- during the request that issues it.
CREATE TABLE IF NOT EXISTS password_reset_tokens (
    token_hash text PRIMARY KEY,
    user_id    uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    -- Denormalised so the confirm path can scope its UPDATE by tenant without a second lookup;
    -- every tenant-scoped write in this codebase carries the tenant.
    tenant_id  uuid NOT NULL,
    created_at text NOT NULL,
    expires_at text NOT NULL,
    -- Set the moment a token is spent. Single-use is what stops a leaked inbox, a shared browser,
    -- or a forwarded email from being replayed into a second takeover later.
    used_at    text
);

-- The request path invalidates a user's outstanding tokens before issuing a new one.
CREATE INDEX IF NOT EXISTS password_reset_tokens_user_idx ON password_reset_tokens (user_id);
-- Lets the sweep find expired rows without a full scan.
CREATE INDEX IF NOT EXISTS password_reset_tokens_expires_idx ON password_reset_tokens (expires_at);
