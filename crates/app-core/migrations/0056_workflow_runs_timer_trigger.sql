-- Allow 'timer' as a workflow run trigger (card #86, Delay Trigger's trailing edge).
--
-- 0015 pinned trigger_source to ('manual','cron','event'). #86 added TriggerSource::Timer —
-- the run a durable timer forces at one node to deliver a DelayTrigger's `stop` — but shipped
-- no migration, so every trailing-edge run was rejected at insert:
--
--   new row for relation "workflow_runs" violates check constraint "workflow_runs_trigger_chk"
--
-- The failure was silent in the worst way. insert_run only logs, so the run still executed; but
-- with no run row, every one of its node logs then failed workflow_run_logs' FK to it. The
-- feature's own diagnostics were being deleted by the same bug that broke it, so Runs showed
-- nothing at all — not a failed stop, just no stop.
ALTER TABLE workflow_runs DROP CONSTRAINT IF EXISTS workflow_runs_trigger_chk;
ALTER TABLE workflow_runs
    ADD CONSTRAINT workflow_runs_trigger_chk
    CHECK (trigger_source IN ('manual', 'cron', 'event', 'timer'));
