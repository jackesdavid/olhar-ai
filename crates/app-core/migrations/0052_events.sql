-- Agnostic event stream (card #66) — one feed every subsystem registers into.
--
-- Today "things that happened" are scattered across three half-overlapping systems: the legacy
-- `alerts` model, the `tenant_events` type registry (workflow fan-out), and `platform_logs` (a
-- low-level debug/audit trace). This is the unified, user-facing, plan-retained stream: alerts,
-- presence, clip lifecycle, auth, custom workflow events — each a first-class, precisely
-- timestamped, entity-linked occurrence. It is also the substrate a future Live "time-travel"
-- timeline needs (events pinned to the right playback moment), which is why the schema carries
-- entity refs (camera / clip_id / room / device / graph / user) up front.
--
-- `platform_logs` stays as the debug trace; this is the product timeline. Phase 1 ships the core
-- + a feed; producers (starting with alerts) migrate onto it next.

CREATE TABLE IF NOT EXISTS events (
    id          bigserial PRIMARY KEY,
    tenant_id   uuid   NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    kind        text   NOT NULL,                    -- catalog slug: alert, presence, clip.started, …
    source      text   NOT NULL,                    -- system | user | workflow | edge
    severity    text   NOT NULL DEFAULT 'info',     -- debug | info | warn | error
    title       text,
    message     text,
    -- entity refs for timeline linkage (all nullable; the Live view scrubs by camera + occurred_at)
    camera      text,
    clip_id     text,
    room_id     bigint,
    room_name   text,
    device_id   text,
    graph_id    bigint,
    user_id     uuid,
    data        jsonb,                              -- typed payload
    occurred_at text   NOT NULL,                    -- RFC3339 — the precise moment it happened
    created_at  text   NOT NULL                     -- RFC3339 — when the row was written
);

-- Newest-first feed, per tenant.
CREATE INDEX IF NOT EXISTS events_tenant_id_idx     ON events (tenant_id, id DESC);
-- Filtered by kind (the feed's kind filter, and "alerts only").
CREATE INDEX IF NOT EXISTS events_tenant_kind_idx   ON events (tenant_id, kind, id DESC);
-- The time-travel query: a camera's events over a playback window.
CREATE INDEX IF NOT EXISTS events_tenant_camera_idx ON events (tenant_id, camera, occurred_at DESC);
-- Whole-tenant timeline by time.
CREATE INDEX IF NOT EXISTS events_tenant_time_idx   ON events (tenant_id, occurred_at DESC);
