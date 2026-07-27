-- Retire the `box_logs` table (card #145, Phase 4). The edge log relay (`/config/logs`,
-- `box_logs_push`) now feeds the unified log queue, which ships to Datadog when `DD_API_KEY` is set
-- (else the `platform_logs` fallback) — nothing has read or written `box_logs` since #145 Phase 3
-- removed `store::insert_box_logs`/`list_box_logs`/`prune_box_logs` and the in-app per-box log viewer.
-- The table was only ever an append-only firehose (pruned on ingest, no pg_cron sweep), so dropping
-- it reclaims write + vacuum load with no reader to break.
--
-- `platform_logs` is deliberately KEPT: the workflow editor's per-run log panel still reads it
-- (`GET /platform-logs`, filtered by trace_id) — retiring that is a separate product decision.
DROP TABLE IF EXISTS box_logs CASCADE;
