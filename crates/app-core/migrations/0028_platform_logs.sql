-- Unified platform audit log — one table for all modules (workflow, edge, presence, device, alert).
-- Replaces the silos of workflow_run_logs + box_logs with a single normalized stream.
-- module-specific data lives in payload (jsonb); trace_id links entries across modules.

CREATE TABLE platform_logs (
  id          bigserial PRIMARY KEY,
  tenant_id   uuid    NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
  module      text    NOT NULL,          -- 'workflow' | 'edge' | 'presence' | 'device' | 'auth' | 'alert'
  trace_id    uuid,                      -- propagated from triggering parent; NULL = root event
  entity_id   text,                      -- e.g. graph_id, box_id, device_id (as text)
  entity_type text,                      -- 'workflow_graph' | 'edge_box' | 'device' | ...
  level       text    NOT NULL DEFAULT 'info',  -- 'debug' | 'info' | 'warn' | 'error'
  message     text    NOT NULL,
  payload     jsonb,                     -- module-specific structured data
  created_at  text    NOT NULL           -- RFC3339 set by app (now_rfc3339()), not timestamptz
);

-- Hot path: tenant + recency (all-module view, newest-first)
CREATE INDEX platform_logs_tenant_idx        ON platform_logs (tenant_id, id DESC);
-- Per-module query + recency
CREATE INDEX platform_logs_tenant_module_idx ON platform_logs (tenant_id, module, id DESC);
-- Trace linkage across modules (e.g. show all logs for a workflow run)
CREATE INDEX platform_logs_trace_idx         ON platform_logs (trace_id) WHERE trace_id IS NOT NULL;
-- Entity drill-down (e.g. one graph's week of logs)
CREATE INDEX platform_logs_entity_idx        ON platform_logs (tenant_id, entity_type, entity_id, id DESC)
  WHERE entity_id IS NOT NULL;

-- TTL: delete entries older than 30 days daily at 03:00 UTC via pg_cron.
-- Wrapped in a DO block so the migration succeeds on dev environments without pg_cron.
DO $$
BEGIN
  PERFORM cron.schedule(
    'platform-logs-ttl',
    '0 3 * * *',
    $cron$
      DELETE FROM platform_logs
      WHERE created_at < to_char(
        (NOW() - INTERVAL '30 days') AT TIME ZONE 'UTC',
        'YYYY-MM-DD"T"HH24:MI:SS"Z"'
      )
    $cron$
  );
EXCEPTION WHEN OTHERS THEN
  RAISE NOTICE 'pg_cron not available — skipping TTL schedule: %', SQLERRM;
END;
$$;
