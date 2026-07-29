-- Retention for `work_jobs` — the durable job queue had NO sweep at all.
--
-- Every other high-volume table is bounded (events 0054, platform_logs 0028, workflow_run_logs
-- 0073), but finished jobs were kept forever. A queue row is transient by nature: once `done`,
-- nothing reads it again — the run history that IS a product surface lives in `workflow_runs` /
-- `workflow_run_logs`, not here. So the table only ever grew.
--
-- It grew a lot, because a workflow_event payload carries the triggering FRAME inline: in
-- production this reached 81,945 done jobs / 8.2 GB of payload (8.6 GB on disk), competing with
-- product data in the same database. The companion fix stops *creating* events nobody can handle;
-- this one stops *keeping* the ones we legitimately processed.
--
-- 2 days, not 30: this is a queue, not an audit log. The window exists only to debug a job that
-- failed or ran oddly in the last day or so. `failed` rows are swept on the same clock — the
-- reaper retries within minutes, so a 2-day-old failure is settled, and `last_error` has already
-- been logged.
--
-- `created_at` is RFC3339 TEXT (sqlx is built without chrono/time, so no timestamptz anywhere) —
-- hence the `to_char` comparison, the same idiom as platform-logs-ttl.
--
-- pg_cron is real since 0046 (bundled in deploy/postgres/Dockerfile); scheduled unguarded like the
-- other jobs there so a missing scheduler fails loudly rather than silently skipping retention.

-- The sweep filters on (status, created_at); without this it is a full scan of a multi-GB table.
CREATE INDEX IF NOT EXISTS work_jobs_ttl_idx ON work_jobs (status, created_at);

SELECT cron.schedule(
  'work-jobs-ttl',
  '35 3 * * *',
  $cron$
    DELETE FROM work_jobs
    WHERE status IN ('done', 'failed')
      AND created_at < to_char(
        (NOW() - INTERVAL '2 days') AT TIME ZONE 'UTC',
        'YYYY-MM-DD"T"HH24:MI:SS"Z"'
      )
  $cron$
);

-- Clear the backlog now rather than waiting for the first scheduled run: the whole point is to
-- reclaim the space this table has already taken. Same predicate as the sweep, so this is exactly
-- one early execution of it.
--
-- Autovacuum reclaims the freed pages for reuse; the on-disk file does not shrink without a
-- VACUUM FULL, which takes an ACCESS EXCLUSIVE lock and is therefore deliberately NOT run here —
-- a migration must not block the queue every service depends on.
DELETE FROM work_jobs
WHERE status IN ('done', 'failed')
  AND created_at < to_char(
    (NOW() - INTERVAL '2 days') AT TIME ZONE 'UTC',
    'YYYY-MM-DD"T"HH24:MI:SS"Z"'
  );
