-- Dictionaries (#113): named, tenant-scoped, typed-column TABLES — the structured cousin of the
-- flat string→string dictionary that HTTP Post's headers/fields use. A dictionary has a schema
-- (`columns`: an ordered list of {name, type} where type ∈ Text|Num|Bool) and `rows` (each row an
-- ordered list of cells, one per column, positionally typed by the schema).
--
-- These are FIRST-CLASS reusable entities: a workflow variable of type Table references one by id,
-- so editing the dictionary here propagates to every workflow that reads it. Workflow nodes read
-- and MUTATE rows mid-run; mutations are made atomic + thread-safe across concurrent runs/pods by
-- taking a `SELECT … FOR UPDATE` lock on this single row before rewriting `rows`.
--
-- Timestamps are RFC3339 text (sqlx is built without chrono/time — it can't decode timestamptz).
CREATE TABLE IF NOT EXISTS dictionaries (
    id          UUID PRIMARY KEY,
    tenant_id   UUID NOT NULL,
    name        TEXT NOT NULL,
    columns     JSONB NOT NULL DEFAULT '[]',
    rows        JSONB NOT NULL DEFAULT '[]',
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS dictionaries_tenant_idx ON dictionaries (tenant_id);

-- One name per tenant (case-insensitive) — the picker and references are by id, but a stable name
-- keeps the catalog legible and prevents accidental dupes.
CREATE UNIQUE INDEX IF NOT EXISTS dictionaries_tenant_name_uidx
    ON dictionaries (tenant_id, lower(name));
