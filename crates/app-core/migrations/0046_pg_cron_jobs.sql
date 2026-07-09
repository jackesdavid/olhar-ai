-- Make pg_cron real, and own every pure-SQL periodic job from here.
--
-- Until now `pg_cron` was never installed (the stock `pgvector/pgvector:pg17` image doesn't
-- ship it), so migration 0028's `platform_logs` 30-day TTL and 0045's trial sweep were both
-- swallowed by their `EXCEPTION WHEN OTHERS ... RAISE NOTICE` guards and **never ran**.
-- `deploy/postgres/Dockerfile` now bundles it. Deploy `sentinel-postgres` before control.
--
-- No exception guard here, on purpose: if the scheduler is missing the boot must fail
-- loudly rather than quietly drop the jobs, which is exactly how this went unnoticed.
CREATE EXTENSION IF NOT EXISTS pg_cron;

-- `cron.schedule()` upserts by job name, so re-running is safe.

-- platform_logs retention (originally 0028, never scheduled).
SELECT cron.schedule(
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

-- Trial expiry (originally 0045). Keeps the stored status honest for the master console;
-- the access gate resolves a lapsed trial at read time and never depends on this running.
-- `ended_at::timestamptz` — RFC3339 text only sorts correctly when UTC offsets match.
SELECT cron.schedule(
  'trial-expiry-sweep',
  '7 * * * *',
  $cron$
    UPDATE tenant_subscriptions
    SET status = 'expired'
    WHERE status = 'trialing'
      AND ended_at IS NOT NULL
      AND ended_at::timestamptz < now()
  $cron$
);
