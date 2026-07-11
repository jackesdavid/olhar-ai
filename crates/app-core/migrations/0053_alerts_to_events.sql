-- Retire the legacy `alerts` model — migrate its rows into the agnostic event stream and drop
-- the table (card #66, Phase 2). Every alert producer/consumer now reads and writes `events`
-- (kind = 'alert'): the workflow CreateAlert/DeliverAlert/GetLastAlert nodes, the chat agent's
-- alert tools, and the control `/alerts` endpoints (now a thin view over events). This is the
-- last step — carry the history over, then the table has no reader.
--
-- Mapping: alerts.level → events.severity, deliveries jsonb → events.data.deliveries, created_at
-- → occurred_at (the moment it fired). The old per-tenant bigint ids are not preserved — events
-- get fresh bigserial ids; nothing referenced an alert id across sessions.

INSERT INTO events
    (tenant_id, kind, source, severity, message, room_id, room_name, data, occurred_at, created_at)
SELECT
    tenant_id,
    'alert',
    COALESCE(NULLIF(source, ''), 'system'),
    COALESCE(NULLIF(level, ''), 'info'),
    message,
    room_id,
    room_name,
    jsonb_build_object('deliveries', COALESCE(deliveries, '[]'::jsonb)),
    created_at,
    created_at
FROM alerts;

DROP TABLE IF EXISTS alerts CASCADE;
