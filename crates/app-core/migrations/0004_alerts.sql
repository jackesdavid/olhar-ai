-- Alerts → Postgres (Phase 0: detach single-writer JSON state).
-- The in-memory AlertStore stays the working set; this table is the persistence
-- target when SENTINEL_STATE_BACKEND=postgres. `id` is app-assigned (the in-memory
-- next_id = max+1) for now — Phase 1 switches it to a DB sequence so the OnNewAlert
-- watermark is correct across processes.
CREATE TABLE IF NOT EXISTS alerts (
    id          bigint PRIMARY KEY,
    level       text NOT NULL DEFAULT 'info',   -- debug|info|warn|error
    message     text NOT NULL DEFAULT '',
    room_id     bigint,                          -- map room, if any
    room_name   text,                            -- name at creation (survives rename)
    source      text NOT NULL DEFAULT '',        -- operator|bishop|scanner|workflow…
    created_at  text NOT NULL,                   -- RFC3339 string (matches the struct)
    deliveries  jsonb NOT NULL DEFAULT '[]'      -- [{target,chat_id,at,ok}]
);

CREATE INDEX IF NOT EXISTS alerts_created_at_idx ON alerts (created_at);
