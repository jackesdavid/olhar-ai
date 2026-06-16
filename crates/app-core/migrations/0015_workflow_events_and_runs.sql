-- Cross-graph events + execution visibility.
--
-- Part A: a per-tenant event registry so an EmitEvent in one graph can be discovered
-- and listened for (OnEvent) by any other graph in the same tenant. Events carry a
-- tenant-scoped category and a source ('user' today; 'system' reserved for future
-- system-emitted events like UserLogin under a System-* category — not seeded here).
--
-- Part B: persistent run history + per-run signal logs so the UI can show what is
-- running (trigger source, pod name) and recent runs survive a restart.
--
-- Also retires the OnNewAlert trigger: any persisted graph using it is rewritten to a
-- disabled Manual graph so the removed Rust `Trigger` variant never hits deserialization.

-- ---- event categories (tenant-scoped) --------------------------------------
CREATE TABLE IF NOT EXISTS tenant_event_categories (
    tenant_id  uuid NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    name       text NOT NULL,
    source     text NOT NULL DEFAULT 'user',
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (tenant_id, name),
    CONSTRAINT tenant_event_categories_source_chk CHECK (source IN ('user', 'system'))
);

-- ---- event registry (tenant-scoped) ----------------------------------------
CREATE TABLE IF NOT EXISTS tenant_events (
    tenant_id        uuid NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    name             text NOT NULL,
    category         text NOT NULL DEFAULT 'General',
    source           text NOT NULL DEFAULT 'user',
    description      text NULL,
    created_by_graph bigint NULL,
    created_at       timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (tenant_id, name),
    CONSTRAINT tenant_events_source_chk CHECK (source IN ('user', 'system'))
);

-- ---- run history -----------------------------------------------------------
CREATE TABLE IF NOT EXISTS workflow_runs (
    id                uuid PRIMARY KEY,
    tenant_id         uuid NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    graph_id          bigint NOT NULL,
    graph_name        text NOT NULL,
    trigger_source    text NOT NULL,
    source_graph_id   bigint NULL,
    source_graph_name text NULL,
    event_name        text NULL,
    pod_name          text NOT NULL,
    status            text NOT NULL DEFAULT 'running',
    result_summary    text NULL,
    started_at        text NOT NULL,                   -- RFC3339 (matches alerts.created_at)
    ended_at          text NULL,
    CONSTRAINT workflow_runs_trigger_chk CHECK (trigger_source IN ('manual', 'cron', 'event')),
    CONSTRAINT workflow_runs_status_chk CHECK (status IN ('running', 'ok', 'failed'))
);
CREATE INDEX IF NOT EXISTS workflow_runs_tenant_graph_idx ON workflow_runs (tenant_id, graph_id, started_at DESC);
CREATE INDEX IF NOT EXISTS workflow_runs_tenant_status_idx ON workflow_runs (tenant_id, status);

-- ---- per-run signal logs ---------------------------------------------------
CREATE TABLE IF NOT EXISTS workflow_run_logs (
    id      bigserial PRIMARY KEY,
    run_id  uuid NOT NULL REFERENCES workflow_runs(id) ON DELETE CASCADE,
    node_id bigint NULL,
    label   text NULL,
    kind    text NOT NULL,
    message text NOT NULL,
    ts      timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT workflow_run_logs_kind_chk CHECK (kind IN ('node', 'log', 'event', 'dispatch', 'trigger', 'skip'))
);
CREATE INDEX IF NOT EXISTS workflow_run_logs_run_idx ON workflow_run_logs (run_id, id);

-- ---- retire the OnNewAlert trigger -----------------------------------------
-- The `Trigger` enum loses its OnNewAlert variant; rewrite any graph still using it to
-- a disabled Manual graph so load_all() can deserialize it. (alert_seen_max in
-- workflow_engine_state is now unused; the table is left in place.)
UPDATE workflows
SET data = jsonb_set(jsonb_set(data, '{trigger}', '"Manual"'::jsonb), '{enabled}', 'false'::jsonb)
WHERE jsonb_typeof(data->'trigger') = 'object' AND data->'trigger' ? 'OnNewAlert';
