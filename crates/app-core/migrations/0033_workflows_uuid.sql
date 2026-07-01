-- Workflow (graph) ids: bigint → uuid.
--
-- Strategy: FRESH RESET (explicit decision) — existing workflows are NOT preserved.
-- Truncate the workflow tables, then retype the id / graph_id columns to uuid.
-- Node ids stay INTEGER: they are a separate per-graph counter that lives inside the
-- graph's JSON blob, unrelated to the graph id.
--
-- Columns retyped: workflows.id (PK), workflow_cron_state.graph_id (PK),
-- workflow_runs.graph_id + source_graph_id (+ its index auto-rebuilds),
-- workflow_node_state.graph_id (PK), tenant_events.created_by_graph (logical ref).
-- Postgres rebuilds PK/indexes automatically when a member column's type changes.

CREATE EXTENSION IF NOT EXISTS pgcrypto; -- gen_random_uuid()

-- CASCADE: workflow_run_logs has an FK to workflow_runs(id), so a bare TRUNCATE of
-- workflow_runs is rejected. CASCADE also empties the referencing run-logs (fine on reset).
TRUNCATE TABLE workflow_runs, workflow_node_state, workflow_cron_state, workflows CASCADE;

ALTER TABLE workflows
    ALTER COLUMN id TYPE uuid USING gen_random_uuid();

ALTER TABLE workflow_cron_state
    ALTER COLUMN graph_id TYPE uuid USING gen_random_uuid();

ALTER TABLE workflow_runs
    ALTER COLUMN graph_id TYPE uuid USING gen_random_uuid();
ALTER TABLE workflow_runs
    ALTER COLUMN source_graph_id TYPE uuid USING (NULL::uuid);

ALTER TABLE workflow_node_state
    ALTER COLUMN graph_id TYPE uuid USING gen_random_uuid();

-- created_by_graph is a logical (unenforced) reference to a graph id; the graphs are
-- gone after the reset, so null it before retyping.
UPDATE tenant_events SET created_by_graph = NULL;
ALTER TABLE tenant_events
    ALTER COLUMN created_by_graph TYPE uuid USING (NULL::uuid);
