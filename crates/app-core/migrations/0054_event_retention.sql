-- Per-tenant retention for the agnostic event stream (card #66, Phase 5).
--
-- Events are the product timeline (alert / presence / clip / workflow / auth), retained per the
-- tenant's plan and swept nightly — the same model as recordings, but its OWN knob. `retention_days`
-- (0047) means "days of cloud recording the plan sells; 0 = no cloud recording"; reusing it would
-- wipe every event for a beta tenant (0 days). Events deserve a floor even on the free tier, so a
-- dedicated column: beta keeps a week, home a month, business a quarter.

ALTER TABLE billing_plans ADD COLUMN IF NOT EXISTS event_retention_days integer NOT NULL DEFAULT 30;

UPDATE billing_plans SET event_retention_days =  7 WHERE id = 'beta';
UPDATE billing_plans SET event_retention_days = 30 WHERE id = 'home';
UPDATE billing_plans SET event_retention_days = 90 WHERE id = 'business';

-- Nightly sweep: delete events past their tenant's plan retention. The window joins each event's
-- tenant → its newest subscription → that plan's `event_retention_days`; a tenant with no
-- subscription falls back to 30 days (COALESCE), never to zero. `occurred_at::timestamptz` is
-- valid because `now_rfc3339()` always carries the local offset (see timeutil).
SELECT cron.schedule(
  'events-retention-sweep',
  '17 3 * * *',
  $cron$
    DELETE FROM events e
    USING (
      SELECT t.id AS tenant_id,
             COALESCE(bp.event_retention_days, 30) AS days
      FROM tenants t
      LEFT JOIN LATERAL (
        SELECT plan_id
        FROM tenant_subscriptions s
        WHERE s.tenant_id = t.id
        ORDER BY s.started_at DESC
        LIMIT 1
      ) sub ON true
      LEFT JOIN billing_plans bp ON bp.id = sub.plan_id
    ) r
    WHERE e.tenant_id = r.tenant_id
      AND e.occurred_at::timestamptz < now() - make_interval(days => r.days)
  $cron$
);
