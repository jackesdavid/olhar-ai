-- Edge command channel (card #144): cloud→edge commands over the edge's OUTBOUND connection, so an
-- edge the cloud cannot reach inbound — a phone/Android on-device edge with no tailnet, and
-- eventually every edge once the tailnet is retired — can still be told to record/snapshot/etc.
--
-- A command is durable so it survives the edge being briefly disconnected (iOS foreground drop, a
-- pod restart) and is redelivered on reconnect. Delivery is **box-scoped**: only the control pod
-- that holds THIS box's live SSE stream claims and pushes it — the connection, not the caller,
-- selects the deliverer, which is what makes it multi-pod correct. The edge runs the command
-- locally and POSTs the result back, filling `result`/`error` and waking the caller that is waiting.
--
-- Timestamps are RFC3339 text (sqlx has no timestamptz decoder; use app_core::timeutil::now_rfc3339).
-- UNLOGGED like work_jobs: faster writes, survives a pod restart, truncated only by a Postgres
-- crash — and a lost command merely degrades to "not delivered", which the caller times out on. Safe.

CREATE UNLOGGED TABLE IF NOT EXISTS edge_commands (
    id          uuid    PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id   uuid    NOT NULL,
    box_id      uuid    NOT NULL,
    cmd         text    NOT NULL,
    args        jsonb   NOT NULL DEFAULT '{}'::jsonb,
    status      text    NOT NULL DEFAULT 'ready',   -- ready | running | done | failed
    attempts    integer NOT NULL DEFAULT 0,
    result      jsonb,
    error       text,
    created_at  text    NOT NULL,
    updated_at  text    NOT NULL,
    run_after   text    NOT NULL,
    expires_at  text    NOT NULL,
    CONSTRAINT edge_commands_status_chk CHECK (status IN ('ready','running','done','failed'))
);

-- The holder pod claims ready (or stale-running) commands for the boxes it holds, oldest first.
CREATE INDEX IF NOT EXISTS edge_commands_box_idx
    ON edge_commands (box_id, status, run_after);

-- The waiting caller polls one command by id for its terminal result.
CREATE INDEX IF NOT EXISTS edge_commands_id_status_idx
    ON edge_commands (id, status);

-- The edge's live command-stream session, so a caller on ANY pod/service can tell whether the box
-- is reachable over SSE (→ enqueue a command) vs. only over the tailnet (→ the legacy inbound POST).
-- `sse_pod` is which pod holds it (observability); `sse_last_seen` is refreshed on connect + a
-- periodic heartbeat, and cleared on disconnect.
ALTER TABLE tenant_boxes ADD COLUMN IF NOT EXISTS sse_last_seen text;
ALTER TABLE tenant_boxes ADD COLUMN IF NOT EXISTS sse_pod       text;
