-- Atomic claim for the generic work queue (unify all async work onto `work_jobs`).
--
-- The queue started idempotent-only: clip-delete consumers could double-process harmlessly, so
-- `claim` took no lock. The next categories moving onto it — workflow events, then notification
-- sends — are **not** idempotent (firing a workflow twice, sending a Telegram twice is wrong), so
-- the claim must hand a job to exactly one consumer. That needs an intermediate `running` status
-- and a due-time so retries can be scheduled (backoff) rather than re-run immediately.
--
-- A `running` job whose consumer crashed would otherwise be stranded (the bug the old
-- `event_queue` has with `claimed`). The claim also reaps `running` rows whose `updated_at` is
-- older than a generous staleness window, so a crash self-heals on the next pass.

-- 'running' = claimed by a consumer, in flight.
ALTER TABLE work_jobs DROP CONSTRAINT IF EXISTS work_jobs_status_chk;
ALTER TABLE work_jobs ADD CONSTRAINT work_jobs_status_chk
    CHECK (status IN ('pending', 'running', 'done', 'failed'));

-- Due time: a job is claimable once run_after <= now. Defaults to created_at (immediately due);
-- backoff/retry bumps it forward. Backfill existing rows so the claim's ORDER BY has a value.
ALTER TABLE work_jobs ADD COLUMN IF NOT EXISTS run_after text;
UPDATE work_jobs SET run_after = created_at WHERE run_after IS NULL;

-- Claim scans due pending jobs by (category, kind), oldest due first.
DROP INDEX IF EXISTS work_jobs_claim_idx;
CREATE INDEX IF NOT EXISTS work_jobs_claim_idx
    ON work_jobs (category, kind, run_after) WHERE status = 'pending';

-- Reaper scans stale `running` jobs (updated_at older than the staleness window).
CREATE INDEX IF NOT EXISTS work_jobs_running_idx
    ON work_jobs (updated_at) WHERE status = 'running';
