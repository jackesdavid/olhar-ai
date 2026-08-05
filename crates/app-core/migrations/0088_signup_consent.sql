-- #167: record WHO agreed to WHAT, WHEN. Consent you cannot evidence is consent you do not have.
--
-- The product is cameras. Video of people's homes is sensitive personal data under both GDPR and
-- LGPD, and the face pipeline (#117) stores biometric templates — a special category under both.
-- Taking consumer money for that without a recorded, versioned acceptance is the gap this closes.
--
-- One row per (user, document) acceptance, never updated: a new version of the Terms produces a NEW
-- row rather than overwriting the old one, so the history of what a given user actually agreed to
-- survives. That is the whole evidentiary value — an UPDATE would destroy it.
--
-- `version` is the document's date stamp (see `app_core::legal::TERMS_VERSION`), not a hash: the
-- published document is the artifact a regulator or a court would ask for, and it is versioned in
-- git alongside this row.
--
-- `ip` / `user_agent` are best-effort context, nullable on purpose — a missing proxy header must
-- never be a reason to refuse a signup. Timestamps are RFC3339 text (sqlx has no chrono/time
-- feature — CLAUDE.md).
CREATE TABLE IF NOT EXISTS user_consents (
    id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id     uuid NOT NULL,
    -- Nullable: consent is given by a PERSON. The tenant is context, and a future flow (an invited
    -- user accepting terms before being placed in a workspace) may not have one yet.
    tenant_id   uuid,
    document    text NOT NULL,   -- 'terms' | 'privacy'
    version     text NOT NULL,   -- e.g. '2026-08-04'
    accepted_at text NOT NULL,   -- RFC3339
    ip          text,
    user_agent  text
);

CREATE INDEX IF NOT EXISTS user_consents_user_idx ON user_consents (user_id);
CREATE INDEX IF NOT EXISTS user_consents_doc_idx  ON user_consents (document, version);
