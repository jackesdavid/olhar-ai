-- Per-account login throttling.
--
-- `POST /auth/login` had no rate limit and no lockout of any kind: an attacker could try passwords
-- against a known address as fast as the network allowed, forever. That is how the seeded default
-- credential would have been found by anyone who thought to look.
--
-- This is the ACCOUNT half. The IP half belongs at the reverse proxy, where it sits in front of
-- every replica; an in-process counter (the one `/enroll` uses) resets on deploy and is per-pod.
-- Neither substitutes for the other: a proxy limit does nothing against credential stuffing spread
-- across many source addresses at one account, and an account lock does nothing against a spray
-- across many accounts from one address.
--
-- On `users` rather than a side table: the row is already read by `find_user_by_email` on every
-- login attempt, so the counter costs no extra query on the hot path.
--
-- `locked_until` is RFC3339 text like every other timestamp here (sqlx is built without the
-- timestamptz decoder). Comparison is textual, which is sound because both sides are produced by
-- `app_core::timeutil` on the same host — the same rule the rest of the schema follows.
ALTER TABLE users ADD COLUMN IF NOT EXISTS failed_logins integer NOT NULL DEFAULT 0;
ALTER TABLE users ADD COLUMN IF NOT EXISTS locked_until text;
