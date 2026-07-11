-- Generic durable work queue for asynchronous jobs (card #64, Phase 6 — built to grow).
--
-- The system has async work in more than one place today (notification sends, workflow-trigger
-- events), each in its own purpose-built table with its own consumer. This is the *generic* queue
-- they can converge on: one place to enqueue, drain, retry, and — the goal — **monitor every
-- async job in the system**. It starts by owning clip deletion; `notification_queue` and
-- `event_queue` migrate onto it later.
--
-- Taxonomy:
--   • category — the domain the job belongs to ('clip_delete'; later 'notification',
--     'workflow_event', …). Monitoring and consumers filter by this.
--   • kind     — the specific action within the category ('db' | 'edge' | 'cloud'; later 'send').
--   • request_id — groups the jobs of one request, so "is this request finished?" is one query.
--   • payload  — JSONB, so each job is self-contained: it carries everything its consumer needs
--     (a clip job carries recording_id, clip_id, cloud_key). The queue, not the source row, is
--     the durable pointer — even once the row is gone, the cloud job still holds the key.
--
-- Consumers are idempotent (deleting an already-deleted object is success) and claim by
-- (category, kind), oldest first. A crash leaves a job `pending` for the next pass. Timestamps
-- are RFC3339 text (sqlx has no timestamptz decoder; use app_core::timeutil::now_rfc3339()).
--
-- Latency: a producer raises `NOTIFY work_jobs` on enqueue so a LISTENing consumer wakes and
-- drains immediately; a slow poll (fallback) still catches retries and any NOTIFY missed while a
-- consumer was down. So the queue is snappy without a tight busy-loop.
--
-- UNLOGGED: this table is not WAL-logged — writes are faster, and it **survives a pod restart**
-- (Postgres keeps running; that was the concern behind this design). It is truncated only by a
-- Postgres *crash* (kill-9 / power loss), which is rare — and even then the R2 lifecycle expires
-- an orphaned object by its retention tier, so a lost cloud job degrades to lifecycle cleanup,
-- not permanent litter. That trade — a small durability gap in an uncommon failure, against
-- lower write cost on every job — is the deliberate choice here.

CREATE UNLOGGED TABLE IF NOT EXISTS work_jobs (
    id          uuid    PRIMARY KEY DEFAULT gen_random_uuid(),
    request_id  uuid    NOT NULL,
    tenant_id   uuid    NOT NULL,
    category    text    NOT NULL,
    kind        text    NOT NULL,
    status      text    NOT NULL DEFAULT 'pending',   -- pending | done | failed
    payload     jsonb   NOT NULL DEFAULT '{}'::jsonb,
    attempts    integer NOT NULL DEFAULT 0,
    last_error  text,
    created_at  text    NOT NULL,
    updated_at  text    NOT NULL,
    CONSTRAINT work_jobs_status_chk CHECK (status IN ('pending', 'done', 'failed'))
);

-- A consumer claims by (category, kind), oldest open job first.
CREATE INDEX IF NOT EXISTS work_jobs_claim_idx
    ON work_jobs (category, kind, created_at) WHERE status = 'pending';

-- "Is this request finished?" and a job waiting on its siblings both scan by request.
CREATE INDEX IF NOT EXISTS work_jobs_request_idx ON work_jobs (request_id);

-- "Is this recording being deleted right now?" — the list marks such clips as in-progress.
CREATE INDEX IF NOT EXISTS work_jobs_recording_idx
    ON work_jobs ((payload->>'recording_id')) WHERE status <> 'done';

-- Monitoring: everything for a tenant, by category, over time.
CREATE INDEX IF NOT EXISTS work_jobs_monitor_idx ON work_jobs (tenant_id, category, created_at);
