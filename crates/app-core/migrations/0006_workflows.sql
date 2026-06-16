-- Workflow graphs → Postgres (Phase 0).
-- One row per named automation Graph, stored as its serialized JSON blob. The
-- in-memory Workflows store stays the working set; this is the persistence target.
-- Replace-all on each editor mutation mirrors the devices table.
CREATE TABLE IF NOT EXISTS workflows (
    id    bigint PRIMARY KEY,   -- Graph.id
    data  jsonb   NOT NULL      -- the full serialized Graph (nodes, edges, trigger, …)
);
