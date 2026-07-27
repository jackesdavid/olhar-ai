-- 5-day retention for workflow_run_logs (the workflow editor's console + per-run node logs).
--
-- These node logs stay in Postgres on purpose — they're an in-app product surface (the editor
-- console + run-log panel read them live), NOT the debug firehose that #145 moved to Datadog. But
-- they're high-volume (~1M rows accrue fast from motion-triggered graphs), so bound them to the last
-- 5 days: recent enough to debug a run, small enough to stop the table competing with product data.
--
-- TTL on the table's own `ts` column (timestamptz DEFAULT now()) — no join to workflow_runs needed.
-- pg_cron is real since 0046 (bundled in deploy/postgres/Dockerfile); scheduled unguarded like the
-- other jobs there so a missing scheduler fails loudly rather than silently skipping retention.
CREATE INDEX IF NOT EXISTS workflow_run_logs_ts_idx ON workflow_run_logs (ts);

SELECT cron.schedule(
  'workflow-run-logs-ttl',
  '15 3 * * *',
  $cron$
    DELETE FROM workflow_run_logs WHERE ts < now() - interval '5 days'
  $cron$
);
