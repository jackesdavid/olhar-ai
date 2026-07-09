-- Trial expiry sweep. Nothing pushes an event when a 30-day trial lapses — the trial
-- exists only here; the payment provider never knew about it. So a periodic sweep flips
-- `trialing → expired` once `ended_at` has passed.
--
-- This is the *reporting* layer only. The access gate never trusts the stored status: it
-- resolves a lapsed trial at read time (`subscriptions::current`), so there is no window
-- between the trial ending and the sweep running.
--
-- pg_cron (not a control-side thread) so it runs exactly once regardless of how many
-- control replicas are up. `ended_at` is RFC3339 text; the cast to timestamptz is exact.
DO $$
BEGIN
  PERFORM cron.schedule(
    'trial-expiry-sweep',
    '7 * * * *',   -- hourly, at :07
    $cron$
      UPDATE tenant_subscriptions
      SET status = 'expired'
      WHERE status = 'trialing'
        AND ended_at IS NOT NULL
        AND ended_at::timestamptz < now()
    $cron$
  );
EXCEPTION WHEN OTHERS THEN
  RAISE NOTICE 'pg_cron not available — skipping trial-expiry schedule: %', SQLERRM;
END;
$$;
