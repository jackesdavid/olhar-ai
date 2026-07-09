-- Cloud recording: retention + honest storage billing (card #64).
--
-- Two corrections to the storage line, independent of whether cloud recording ships:
--
--   1. It billed bytes we don't store. `recordings` rows describe clips on the *edge's* own
--      disk; the invoice printed "storage — N GB" for footage that never left the customer's
--      hardware. Harmless only while the rate is 0. Only cloud-resident clips count now.
--   2. It summed ingest, not occupancy. Storage is a **stock**: a tenant keeping 30 GB for
--      60 days costs 20x one keeping it 3 days, yet both were charged the same. Retention —
--      the thing we actually pay for — never reached the bill. `tenant_storage_daily` is
--      sampled nightly so the invoice can charge GB-month (the average, not the sum).
--
-- It also settles where money lives. Migration 0044 moved prices to `plan_prices`
-- (plan, currency); the overage *rates* are money too and belong there. Quotas stay on the
-- tier: a GB is a GB in any currency, but a rate is not.

-- ---- where a clip lives -------------------------------------------------------------
ALTER TABLE recordings ADD COLUMN IF NOT EXISTS cloud_key         text;
ALTER TABLE recordings ADD COLUMN IF NOT EXISTS cloud_uploaded_at text;
ALTER TABLE recordings ADD COLUMN IF NOT EXISTS upload_state      text NOT NULL DEFAULT 'pending';
ALTER TABLE recordings DROP CONSTRAINT IF EXISTS recordings_upload_state_chk;
ALTER TABLE recordings ADD CONSTRAINT recordings_upload_state_chk
    CHECK (upload_state IN ('pending', 'uploading', 'uploaded', 'failed'));

-- The nightly sampler and the invoice both scan by this.
CREATE INDEX IF NOT EXISTS recordings_cloud_idx
    ON recordings (tenant_id) WHERE cloud_key IS NOT NULL;

-- ---- retention is the product; the quota is the guardrail ---------------------------
-- Days of cloud retention the plan sells. 0 = no cloud recording.
ALTER TABLE billing_plans ADD COLUMN IF NOT EXISTS retention_days integer NOT NULL DEFAULT 0;

-- Money moves to `plan_prices` (per currency). Quotas (`included_*`) stay on the tier.
ALTER TABLE plan_prices ADD COLUMN IF NOT EXISTS overage_gpu_per_sec       double precision NOT NULL DEFAULT 0;
ALTER TABLE plan_prices ADD COLUMN IF NOT EXISTS overage_storage_per_gb    double precision NOT NULL DEFAULT 0;
ALTER TABLE plan_prices ADD COLUMN IF NOT EXISTS overage_notification_each double precision NOT NULL DEFAULT 0;

ALTER TABLE billing_plans DROP COLUMN IF EXISTS overage_gpu_per_sec;
ALTER TABLE billing_plans DROP COLUMN IF EXISTS overage_storage_per_gb;
ALTER TABLE billing_plans DROP COLUMN IF EXISTS overage_notification_each;

-- Retention + quota. Normal motion use lands at 9% / 26% / 56% of quota, so an overage line
-- only ever appears for a tenant driving the edge to record continuously from a workflow.
UPDATE billing_plans SET retention_days =  3, included_storage_gb =   5 WHERE id = 'beta';
UPDATE billing_plans SET retention_days = 30, included_storage_gb = 100 WHERE id = 'home';
UPDATE billing_plans SET retention_days = 60, included_storage_gb = 750 WHERE id = 'business';

-- ~3.9x our R2 cost (R$ 0,077 / GB-month). Beta is free: a 3-day, 2-camera trial cannot run
-- up a bill worth collecting, and charging a trial is a bad first impression.
UPDATE plan_prices SET overage_storage_per_gb = 0.30 WHERE plan_id IN ('home','business') AND currency = 'BRL';
UPDATE plan_prices SET overage_storage_per_gb = 0.06 WHERE plan_id IN ('home','business') AND currency = 'USD';

-- ---- GB-month: storage is a stock, so sample it ------------------------------------
CREATE TABLE IF NOT EXISTS tenant_storage_daily (
    tenant_id uuid   NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    day       date   NOT NULL,
    bytes     bigint NOT NULL,
    PRIMARY KEY (tenant_id, day)
);

-- Every tenant gets a row every day, zeros included: an average over only the non-zero days
-- would overstate a tenant who deleted their footage mid-month.
SELECT cron.schedule(
  'storage-daily-sample',
  '23 3 * * *',
  $cron$
    INSERT INTO tenant_storage_daily (tenant_id, day, bytes)
    SELECT t.id, CURRENT_DATE, COALESCE(SUM(r.size_bytes), 0)
    FROM tenants t
    LEFT JOIN recordings r
      ON r.tenant_id = t.id AND r.cloud_key IS NOT NULL AND r.upload_state = 'uploaded'
    GROUP BY t.id
    ON CONFLICT (tenant_id, day) DO UPDATE SET bytes = EXCLUDED.bytes
  $cron$
);
