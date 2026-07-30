-- Session revocation, for the central auth service.
--
-- A JWT is self-validating, which is its point and also its problem: until now the ONLY way to
-- invalidate a session was rotating JWT_SECRET, which logs out every user of every tenant at once.
-- That is a blunt instrument for "this person left the company" or "this laptop was stolen", and it
-- is exactly the bind §0 is in right now — the exposed secret cannot be rotated without a
-- platform-wide logout.
--
-- `sessions_valid_from` is a per-user cutoff: any token issued BEFORE it is refused. Setting it to
-- now() logs that one user out everywhere, immediately, without touching anyone else. NULL (the
-- default, and the state every existing row starts in) means no revocation, so this migration
-- changes nobody's session.
--
-- Deliberately UNIX SECONDS, not the house RFC3339 text convention. It is compared against the
-- token's `iat`, which is unix seconds because that is what JWT specifies — the same exception
-- app_core::signing documents for its timestamps. Storing text here would mean parsing on every
-- validation, and a parse that fails while gating someone's access is a decision about whether to
-- fail open or lock them out permanently. Integers have no such failure mode.
--
-- Revoking a SINGLE session (rather than all of a user's) needs a per-token id; `iat` cannot
-- distinguish two tokens minted in the same second. That is a later addition (a `jti` claim plus a
-- revoked-ids table) and is not needed for the case this exists to serve.
ALTER TABLE users ADD COLUMN IF NOT EXISTS sessions_valid_from bigint;

COMMENT ON COLUMN users.sessions_valid_from IS
  'Unix seconds. Tokens with iat < this are refused by /internal/introspect. NULL = no revocation.';
