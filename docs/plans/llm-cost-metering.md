# The assistant spends money and nothing records it

**Board card:** #193 Meter what the AI assistant costs, per tenant
**Goal:** Every Anthropic call is attributed to a tenant with its real token cost, so the assistant
can appear in a bill, in a quota, and in a margin calculation. Today it appears in none of them.

## The gap, precisely

`crates/app-core/src/agent/client.rs` parses the response and reads `content` and `stop_reason`.
The word `usage` appears in that file **zero times** — and every Anthropic response carries it.

Three things make the true cost bigger than it looks:

1. **One chat turn is up to six API calls.** `MAX_TOOL_ITERS = 6`, and each iteration is a full
   request carrying the whole conversation so far. A single user question that uses tools costs
   several calls, each re-sending a growing message list.
2. **Prompt caching is already on.** The system block carries `cache_control: ephemeral`, so the
   response reports **four** counters, not two: `input_tokens`, `output_tokens`,
   `cache_creation_input_tokens`, `cache_read_input_tokens`. Cache reads are a fraction of the input
   price and cache writes cost more than plain input. Modelling only input+output gets the number
   wrong in both directions.
3. **`ANTHROPIC_MODEL` is a per-deployment env var** (`agent/domain.rs`), defaulting to
   `claude-opus-4-8`. A deployment that switches model silently changes the cost per token, so a
   rate hard-coded anywhere but next to the model name will lie the first time someone edits an env.

Measured evidence of the size: one ordinary session cost roughly **US$0.05** on the operator's own
account. Against a home plan, a hundred such exchanges a month is a material share of the margin.

## What already exists to build on

`billing::record_ai_usage` + the `ai_usage` table are exactly the right *shape* — per-tenant,
append-only, `op` string, rate snapshot, computed cost, and a deliberate rule that a metering write
must never fail the operation it measures. But the columns are time (`delay_ms`, `exec_ms`,
`rate_per_sec`), because it was built for the RunPod GPU sidecar. Tokens are not time.

**Proposed: a sibling table, not a widened one.** `llm_usage` with the four token counters, the
model string, the rate snapshot and the computed cost. Widening `ai_usage` would leave every GPU row
with four null token columns and every LLM row with two meaningless millisecond columns, and the
first person to write a cost query would have to know which half to ignore.

## Phase 1 — record it

- `llm_usage(id, tenant_id, op, model, input_tokens, output_tokens, cache_write_tokens,
  cache_read_tokens, rate_snapshot_json, cost, created_at)`, additive migration.
- `client.rs` reads `parsed["usage"]` after each iteration and records **per call**, not per turn —
  a turn that burned six calls should read as six rows, or the tool loop stays invisible.
- Rates live in a table keyed by model, seeded in code, so changing `ANTHROPIC_MODEL` without a rate
  produces a row with `cost = 0` **and a loud log line**, never a silently free call.
- The write must not fail the reply. Same rule as `record_ai_usage`: log and continue.

**What cannot be measured, stated plainly:** a request that times out on our side may still have
been produced and billed by Anthropic. We record what a response reports; anything else is a
reconciliation problem against the provider invoice, not something this table can solve.

## Phase 2 — make it visible

- The tenant's own usage page shows assistant spend next to storage and notifications.
- The master console shows spend per tenant, because the first question when a bill looks wrong is
  "who is doing this?"
- Reconciliation check: monthly sum of `llm_usage.cost` against the Anthropic invoice. A drift
  bigger than a few percent means the tool loop is doing something nobody modelled.

## Phase 3 — a cap that degrades instead of surprising

A per-tenant monthly ceiling. On reaching it the assistant says so and stops calling the model —
it does not silently fail, and it does not keep spending. Which ceiling belongs to which plan is
#194's decision; the mechanism belongs here.

## Verification criteria

1. One assistant exchange that uses a tool produces **more than one** `llm_usage` row.
2. The four token counters are non-zero where the response reported them; a cached turn shows
   `cache_read_tokens > 0`.
3. Summed cost for a known session is within a few percent of the provider's own figure for it.
4. Setting `ANTHROPIC_MODEL` to a model with no rate row logs an error and records `cost = 0` —
   it does not guess a rate.
5. Killing the database mid-conversation does not break the reply.

## Out of scope

- Charging for it. That is #194, and it needs the numbers this card produces first.
- The GPU/face sidecar metering, which already works.
