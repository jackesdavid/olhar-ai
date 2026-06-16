-- Workflow engine state → Postgres (Phase 0.5).
-- The engine's timing state was RAM-only, so a restart replayed the OnNewAlert
-- backlog and fired long crons immediately on boot. Persisting it here lets the
-- adapter restore the watermark + cron timers so neither happens. The in-flight
-- run counters stay RAM-only by nature (runs die with the process).

-- Global single-row state: the new-alert high-water mark. The boolean PK + CHECK
-- enforce at most one row (a classic singleton-table guard).
CREATE TABLE IF NOT EXISTS workflow_engine_state (
    id              boolean PRIMARY KEY DEFAULT true,
    alert_seen_max  bigint  NOT NULL DEFAULT 0,
    CONSTRAINT workflow_engine_state_singleton CHECK (id)
);

-- Per-graph cron timer: when each cron graph last fired, as unix epoch millis
-- (wall-clock — Instant can't survive a restart). On boot the adapter converts
-- this back into a monotonic anchor so a mid-interval restart waits out the
-- remainder instead of firing immediately.
CREATE TABLE IF NOT EXISTS workflow_cron_state (
    graph_id       bigint PRIMARY KEY,
    last_fired_ms  bigint NOT NULL
);
