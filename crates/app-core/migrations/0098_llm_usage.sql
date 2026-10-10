-- Card #193 — meter what the assistant costs, per tenant.
--
-- `ai_usage` (0039) is the same SHAPE but measured in milliseconds: it was built for the RunPod
-- GPU sidecar, where cost = seconds x rate. Tokens are not time. Widening that table would leave
-- every GPU row with four null token columns and every LLM row with two meaningless millisecond
-- ones, and the first person to write a cost query would have to know which half to ignore.

CREATE TABLE IF NOT EXISTS llm_usage (
    id                  uuid PRIMARY KEY,
    tenant_id           uuid NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    -- What was being done ('chat', later: 'summarize', 'classify'), so spend can be attributed to
    -- a feature and not just to a tenant.
    op                  text NOT NULL,
    model               text NOT NULL,
    -- FOUR counters, not two. Prompt caching is on (the system block carries cache_control), so
    -- Anthropic reports cache creation and cache reads separately from plain input. Cache reads
    -- cost a fraction of input and cache writes cost MORE than input, so modelling only
    -- input+output gets the number wrong in both directions.
    input_tokens        integer NOT NULL DEFAULT 0,
    output_tokens       integer NOT NULL DEFAULT 0,
    cache_write_tokens  integer NOT NULL DEFAULT 0,
    cache_read_tokens   integer NOT NULL DEFAULT 0,
    -- The rates used, snapshotted. A cost row that cannot be recomputed cannot be audited when the
    -- provider invoice disagrees.
    rate_snapshot_json  text,
    cost                double precision NOT NULL DEFAULT 0,
    created_at          text NOT NULL
);

CREATE INDEX IF NOT EXISTS llm_usage_tenant_created_idx ON llm_usage (tenant_id, created_at DESC);

-- Rates live in a TABLE, not in code or an env var: prices are money, they change, and the model
-- is chosen per deployment by ANTHROPIC_MODEL. A rate hard-coded anywhere but next to the model
-- name lies the first time someone edits an env.
CREATE TABLE IF NOT EXISTS llm_rates (
    model                   text PRIMARY KEY,
    -- USD per MILLION tokens, matching how the provider publishes them, so a human comparing this
    -- table to the price page is comparing like with like.
    input_per_mtok          double precision NOT NULL,
    output_per_mtok         double precision NOT NULL,
    cache_write_per_mtok    double precision NOT NULL,
    cache_read_per_mtok     double precision NOT NULL,
    updated_at              text NOT NULL
);

-- Published rates, 2026-08-18. Cache write = 1.25x input (5-minute TTL); cache read = 0.1x input.
INSERT INTO llm_rates (model, input_per_mtok, output_per_mtok, cache_write_per_mtok, cache_read_per_mtok, updated_at)
VALUES
    ('claude-opus-4-8',   5.0,  25.0,  6.25, 0.50, '2026-08-18T00:00:00Z'),
    ('claude-opus-5',     5.0,  25.0,  6.25, 0.50, '2026-08-18T00:00:00Z'),
    ('claude-opus-4-7',   5.0,  25.0,  6.25, 0.50, '2026-08-18T00:00:00Z'),
    ('claude-sonnet-5',   3.0,  15.0,  3.75, 0.30, '2026-08-18T00:00:00Z'),
    ('claude-sonnet-4-6', 3.0,  15.0,  3.75, 0.30, '2026-08-18T00:00:00Z'),
    ('claude-haiku-4-5',  1.0,   5.0,  1.25, 0.10, '2026-08-18T00:00:00Z'),
    ('claude-fable-5',   10.0,  50.0, 12.50, 1.00, '2026-08-18T00:00:00Z')
ON CONFLICT (model) DO NOTHING;
