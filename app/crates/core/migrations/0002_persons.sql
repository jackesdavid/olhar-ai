-- Person module: the people the system recognizes/manages, plus a generic
-- superior→subordinate hierarchy and per-superior permissions. Domain-agnostic
-- (parent→child in a school, boss→employee in a company).
CREATE TABLE IF NOT EXISTS persons (
    id              text PRIMARY KEY,
    name            text NOT NULL DEFAULT '',
    -- superior in the hierarchy (a guardian/parent/boss). NULL = top-level.
    parent_id       text REFERENCES persons(id) ON DELETE SET NULL,
    -- this person's own notification target (recipient label); NULL = none.
    recipient_label text,
    -- actions this person (as a superior) may perform over its subtree
    -- (e.g. view_photo, view_location, view_report, receive_notifications).
    permissions     text[] NOT NULL DEFAULT '{}',
    -- facial biometric embedding, captured at enrollment (Phase 2). NULL until then.
    embedding       vector(512),
    -- explicit biometric consent, set when an embedding is enrolled.
    consent         boolean NOT NULL DEFAULT false,
    created_at      timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS persons_parent_id_idx ON persons (parent_id);
