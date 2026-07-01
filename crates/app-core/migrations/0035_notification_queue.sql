-- Durable queues for the notification service (card #57).
--
-- One claim-and-process pattern, two queues, opposite consumers:
--   • notification_queue — outbound sends. Producer = flow (RemoteNotifier);
--     consumer = sentinel-notification.
--   • event_queue        — inbound triggers that fire workflows. Producers = notification
--     (telegram_message), control (motion, edge:full-disk); consumer = sentinel-flow.
-- Consumers claim due rows with `SELECT … FOR UPDATE SKIP LOCKED`, process, then mark
-- terminal. Timestamps are RFC3339 `text` (sqlx is built without the timestamptz decoder;
-- generate with app_core::alerts::now_rfc3339()).

CREATE TABLE IF NOT EXISTS notification_queue (
    id              uuid PRIMARY KEY,
    tenant_id       uuid NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    status          text NOT NULL DEFAULT 'queued',   -- queued | sending | sent | failed
    attempts        int  NOT NULL DEFAULT 0,
    target          text NULL,                         -- beacon label; NULL = all enabled recipients
    message         text NOT NULL DEFAULT '',
    image_key       text NULL,                         -- object-store key (snapshot JPEG)
    video_key       text NULL,                         -- object-store key (recording mp4)
    result          text NULL,                         -- last send result / error
    created_at      text NOT NULL,                     -- RFC3339
    next_attempt_at text NOT NULL,                     -- RFC3339 (due time; retry backoff bumps it)
    sent_at         text NULL,                         -- RFC3339 (terminal)
    CONSTRAINT notification_queue_status_chk CHECK (status IN ('queued', 'sending', 'sent', 'failed'))
);
-- claim loop: due, non-terminal rows, FIFO by due time
CREATE INDEX IF NOT EXISTS notification_queue_claim_idx ON notification_queue (status, next_attempt_at);
-- observability: tenant-scoped by status over time
CREATE INDEX IF NOT EXISTS notification_queue_tenant_idx ON notification_queue (tenant_id, status, created_at);

CREATE TABLE IF NOT EXISTS event_queue (
    id         uuid PRIMARY KEY,
    tenant_id  uuid NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    event      text NOT NULL,                          -- e.g. "telegram_message", "motion"
    source     text NOT NULL DEFAULT 'system',         -- emitter label
    payload    jsonb NOT NULL DEFAULT '{}',
    status     text NOT NULL DEFAULT 'queued',         -- queued | claimed | done | failed
    attempts   int  NOT NULL DEFAULT 0,
    created_at text NOT NULL,                          -- RFC3339
    claimed_at text NULL,
    CONSTRAINT event_queue_status_chk CHECK (status IN ('queued', 'claimed', 'done', 'failed'))
);
CREATE INDEX IF NOT EXISTS event_queue_claim_idx ON event_queue (status, created_at);
CREATE INDEX IF NOT EXISTS event_queue_tenant_idx ON event_queue (tenant_id, created_at);
