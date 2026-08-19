# Sell the outcome, not the seat

**Board card:** #194 Outcome-based pricing — bill for results the customer got, not capacity
**Goal:** A bill a customer can read as "here is what Sentinel did for me", instead of "here is what
I rented". Two shapes, one engine: home and business.

## Why the current model does not say that

Today a tenant buys a **tier**: `camera_cap`, `retention_days`, and three metered dimensions with
overage — `included_gpu_sec`, `included_storage_gb`, `included_notifications` (`billing/plans.rs`).

That is capacity. Every one of those units is something *we* spend, not something the customer
*got*. A customer whose gate opened 40 times, whose house was watched for a month and who was
notified twice about a stranger has no line on the bill that says any of it.

The metering machinery is fine — `plan_prices` already carries per-unit overage rates and
`ai_usage` proves the append-only ledger pattern works. **The units are what is wrong.**

## The trap to avoid: never bill per event GENERATED

A camera pointed at a busy street produces thousands of motion events. Billing on those means:

- the customer's bill is set by how noisy their street is, not by what they got;
- **we would profit from a worse detector.** Every false positive is revenue. Any work to reduce
  noise — better motion, better face thresholds, #123's margin test — becomes a cost to us.

That incentive is fatal, and it is easy to walk into because raw events are the easiest thing to
count. Bill only on outcomes that someone **acted on or confirmed**.

## Which outcomes we can already prove

| Outcome | Where it is already recorded | Confirmable? |
|---|---|---|
| A gate was opened by an approval | `device_gate_requests` — has `answered_at`, `auth_factor`, state | **yes** — a human confirmed with Face ID or a passcode |
| A person was recognised | faces pipeline + `events` (`kind` catalog) | yes, when it resolves to a labelled person |
| An event was acknowledged | `events` | only once the product HAS an acknowledge action |
| A notification was delivered | notification queue | **NO — see below** |
| A clip was retained | recording + storage metering | yes |

**The one that blocks everything: delivery is not verified.** #190 is open precisely because Expo's
ticket says "queued" and we never read the receipt. On 2026-08-17 every Device Gate push was dying
at Apple while the run log read like the person had ignored it. **Billing for delivered
notifications today would mean charging for messages that never arrived.** #190 is not a nice-to-have
next to this card — it is a precondition for one of the units.

That is the general rule this card should be built on: **an outcome may only be billed if we can
prove it happened.** Anything we cannot prove stays in the flat part of the plan.

## Phase 1 — shadow metering, charge nothing

Record outcome units for every tenant for a full billing month, alongside the current model, and
charge on the current model. Then compare: what would each tenant have paid?

Repricing on units nobody has counted is how a company discovers its new plan is unprofitable one
month after launching it. This phase is cheap — it is a table and a nightly rollup — and it is the
only thing that turns the pricing argument from opinion into arithmetic.

Deliverable: a per-tenant table of "old bill vs simulated new bill", for the operator to read.

## Phase 2 — the two shapes

**Home** — a small flat fee that includes a generous outcome allowance, because a household's
outcome count is naturally bounded (one gate, a few doors, a family). The flat part buys
predictability, which is what a residential customer actually wants; the allowance exists so an
outlier does not cost us money. Overage should be rare enough that hitting it is a signal to move
plan, not a monthly surprise.

**Business** — priced per site, with outcome bundles and a real SLA. A business genuinely varies:
11 trucks, three gates, forty staff. Here the outcome unit is the honest one, and the bundle is
what makes the bill predictable.

Both ride the existing engine: tiers with `included_*` and per-unit overage. This is an extension of
`plans.rs`, not a rewrite.

## Phase 3 — the assistant is a cost centre inside both

#193 makes assistant spend measurable per tenant. Once it is, three options:

1. **Included allowance + overage**, like storage. Simple, matches the rest of the model.
2. **Credits**, bought separately. Clearer for heavy users, worse for casual ones.
3. **Business only.** Home gets a small fixed allowance and a cap that degrades gracefully.

Recommended: **1 for business, 3 for home.** A residential customer should never be able to run up
an assistant bill; a business should be able to buy more when it wants more.

**The number that decides this:** a single ordinary exchange measured at ~US$0.05. At a hundred
exchanges a month that is US$5 per tenant — which against a home plan is a large fraction of the
margin, and against a business plan is noise. That asymmetry is the whole reason home and business
should treat it differently.

## Phase 4 — say it on the invoice

The bill lists outcomes in the customer's words: gates opened, people recognised, alerts confirmed,
days of video kept. The current invoice lists GB and notification counts. Nobody buys GB.

## Verification criteria

1. Shadow month produces, per tenant, old bill vs simulated bill, and the operator can read why they
   differ.
2. No unit is billed that cannot be proven from a stored record with a confirmation on it.
3. A tenant with a noisy camera and no acknowledged events pays the same as a quiet one.
4. Assistant spend appears per tenant and a home tenant cannot exceed the cap.
5. Existing tenants are unaffected until explicitly migrated.

## Open decisions — these are the operator's, not the implementer's

1. **What is THE unit for home?** The gate approval is the most defensible (a human confirmed it
   with biometrics, it is unambiguous, and it is what the customer actually wanted). But a home
   without a gate then has no unit. Alternative: "confirmed alert".
2. **Does the product need an explicit acknowledge action?** Several candidate units depend on a
   human confirming an event, and today only the Device Gate has that. Adding one is product work
   this card would depend on.
3. **Grandfathering.** Existing tenants on the old model — migrate, or run both indefinitely?
4. **Currency of the outcome.** BRL and USD already exist per (plan, currency); outcome rates need
   the same treatment, not a converted number.

## Out of scope
- Changing the payment provider or the invoicing flow (Asaas / #63 stay as they are).
- Any repricing before the shadow month has produced numbers.

## Related
[[llm-cost-metering]] (#193) · [[push-receipts-not-just-tickets]] (#190, a precondition) ·
[[storage-billing-economics]] (#71) · [[tenant-billing-feature]]
