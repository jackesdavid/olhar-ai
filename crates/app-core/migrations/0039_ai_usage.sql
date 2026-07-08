-- Per-tenant AI/GPU usage metering (RunPod Serverless). One append-only row per
-- `ai_service` call: the billed time from the RunPod response envelope (delayTime +
-- executionTime, in ms) and the cost we attribute (billed seconds × rate). RunPod has
-- no per-request cost center — it bills the whole endpoint by GPU-second — so we
-- attribute in-app, knowing the tenant at call time. NEVER updated: immutability is what
-- makes downstream invoices auditable. `rate_per_sec`/`cost` are float8 (sqlx is built
-- without a numeric decoder); timestamps are RFC3339 text (no timestamptz decoder).
CREATE TABLE IF NOT EXISTS ai_usage (
    id           uuid PRIMARY KEY,
    tenant_id    uuid NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    op           text NOT NULL,
    delay_ms     integer NOT NULL DEFAULT 0,
    exec_ms      integer NOT NULL DEFAULT 0,
    success      boolean NOT NULL,
    rate_per_sec double precision NOT NULL DEFAULT 0,
    cost         double precision NOT NULL DEFAULT 0,
    created_at   text NOT NULL
);

CREATE INDEX IF NOT EXISTS ai_usage_tenant_idx ON ai_usage (tenant_id, created_at);
