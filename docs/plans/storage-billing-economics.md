# Storage Billing Economics — R2 cost vs. quota, no-loss guardrails

**Board card:** Storage billing economics — R2 cost vs quota, no-loss guardrails (ToDo)
**Goal:** Prove the cloud-recording storage line never loses money, document exactly where the
quotas and rates come from, and close the one genuine (bounded) leak — unmetered R2 operations.

Related: [[cloud-recording-storage]] (#64, the feature), [[tenant-cloud-cost]] (#63, invoicing).

---

## The three questions this answers

1. **Where does the 100 GB quota come from?**
2. **What do we pay Cloudflare R2 if a Home tenant uses 100 GB for a month?**
3. **Can we lose money anywhere in the calculus?**

---

## 1. Where the quotas come from

Migration `0047_cloud_recording_billing.sql` sets, per tier:

| Tier | `included_storage_gb` | `retention_days` | `base_fee_brl` | overage `/GB-month` |
|------|----------------------|------------------|----------------|---------------------|
| beta | 5 | 3 | 0 | 0 (free) |
| home | **100** | 30 | 39.90 | R$0.30 / $0.06 |
| business | 750 | 60 | 499.00 | R$0.30 / $0.06 |

The quotas are **comfort headroom, not cost-derived break-evens.** The migration comment states
the sizing intent: *"Normal motion use lands at 9% / 26% / 56% of quota"* — i.e. a typical Home
tenant (~6 cameras, motion-triggered) stores ~26 GB, ~26% of 100 GB. 100 GB was chosen so normal
use never trips overage; the overage line only appears for a tenant driving the edge to record
**continuously** from a workflow. Rates live in `plan_prices` (per currency); quotas live on
`billing_plans` (a GB is a GB in any currency — migration 0044/0047 split).

## 2. What 100 GB/month costs us on R2

Cloudflare R2 **Standard** pricing (verified 2026-07, developers.cloudflare.com/r2/pricing):

- Storage: **$0.015 / GB-month**
- Class A ops (writes: PUT / multipart parts / CreateMultipart): **$4.50 / million**
- Class B ops (reads: GET / HEAD): **$0.36 / million**
- **Egress / data transfer: $0 (free)** — the reason R2 beats S3 for video playback
- Free tier (per **account**, not per tenant): 10 GB-month storage, 1M Class A, 10M Class B / month

A Home tenant at a **full 100 GB for the whole month**:

```
storage : 100 GB-month × $0.015          = $1.50 / mo
writes  : ~clips × ~2–3 Class A ops       ≈ $0.00–0.50 / mo (normal)
reads   : playback GETs                   ≈ negligible ($0.36/M)
egress  : free                            = $0
--------------------------------------------------------------
total   ≈ $1.50 – $2.00 / mo  ≈ R$7.70 – R$10 (FX R$5.1–5.5/USD)
```

The code assumes **R$0.077 / GB-month** (0047 comment) = $0.015 at ~R$5.1/USD — consistent.

**Margin at full quota:** revenue R$39.90 vs. cost ~R$8 → storage is ~19% of the base fee,
**~75% gross margin even if the tenant fills 100 GB.** Per tier at full quota:

| Tier | Full-quota R2 cost | Revenue | Storage as % of revenue |
|------|--------------------|---------|-------------------------|
| beta | ~R$0.38 (5 GB) | R$0 (trial) | free by design |
| home | ~R$7.70 (100 GB) | R$39.90 | ~19% |
| business | ~R$57.7 (750 GB) | R$499 | ~12% |

**Overage economics:** we charge R$0.30/GB-month over quota; cost is R$0.077 → **3.9× markup**
(USD: $0.06 vs $0.015 = 4.0×). Profitable on every marginal GB.

## 3. Where money could leak (and whether it's real)

1. **Unmetered R2 operations — the one genuine leak (bounded).** We bill GB-month *occupancy*
   but R2 also charges per **operation**. A workflow recording many tiny short clips inflates
   Class A writes while adding almost nothing to GB-month, so we charge ~nothing and still pay
   ops. Worst realistic case: 1 clip / 10 s = ~259k clips/mo × ~3 Class A ops ≈ $3.50/mo — still
   under $5, but it is the only cost that scales with tenant *behaviour we don't price*. The
   per-account free tier (1M Class A) is consumed collectively, so it vanishes as tenants grow.
   **This is the thing to guard.**

2. **FX drift — large buffer, not a near-term threat.** R2 bills USD; we charge fixed BRL. Cost
   R$0.077/GB holds only at ~R$5.1/USD. At R$7/USD cost rises to R$0.105/GB — still 2.85× under
   the R$0.30 overage rate, and full-quota Home cost rises to only ~R$10.5 vs R$39.90. Safe
   unless BRL collapses past ~R$20/USD. Worth a documented FX assumption, not a code change.

3. **GB-month sampling granularity.** `storage-daily-sample` (pg_cron, 03:23) snapshots once a
   day and the invoice AVGs the month. A tenant who uploads 90 GB just after a sample and deletes
   before the next shows ~0 GB-month → under-billed for ~24h of occupancy + the writes. Bounded
   by retention + quota; minor. A second daily sample would tighten it if ever material.

4. **Quota is billed, not hard-capped.** A runaway recorder can exceed 100 GB unbounded; we bill
   overage (profitable) but carry **collection risk** if they don't pay. Beta can't store
   (retention 0 → no cloud), so the free tier is safe.

5. **Storage class.** We use Standard ($0.015). Infrequent Access is $0.01 storage (−33%) but
   adds $0.01/GB retrieval + a 30-day minimum + 2× ops. For write-once/watch-rarely footage IA
   *might* win; playback-heavy tenants lose. Evaluate, don't assume.

---

## Verdict

At the current ratios the storage line **cannot lose money on storage itself** — every tier keeps
a ≥5× margin at full quota and overage is ~4× cost. The only place a tenant's behaviour can cost
more than we charge is **R2 operations**, and even the pathological case is a few dollars. So this
card is not "fix a loss," it's "instrument and cap the one unpriced dimension before scale makes it
matter."

## Open decisions (grill before building)

1. **Operations: cap or price?** (a) Cap clip creation rate per tenant on the edge (simplest,
   protects R2 directly); (b) fold an estimated ops cost into the GB-month cost model / master
   storage view; (c) do nothing until account-level R2 ops approach the 1M free tier. Recommend
   (a) a soft edge-side floor on clip segment length + (c) a master alert on account ops.
2. **Surface real R2 spend to the master.** Pull actual R2 usage (Cloudflare GraphQL analytics
   API) into the `/admin/storage` console so margin is measured, not assumed. Worth it?
3. **FX policy.** Hard-code the R$/USD assumption in a documented constant and alert if the live
   `fx` rate drifts past a threshold (e.g. R$7/USD), vs. leave the 3.9× buffer absorb it.
4. **Storage class per tier.** Keep everything Standard, or move business (large, cold) footage to
   Infrequent Access and model the retrieval-fee break-even?

## Out of scope (v1)

- Changing quotas or rates (they're proven safe; only instrument).
- Real-time R2 cost metering per clip.
- Per-operation line on the customer invoice (we bill GB-month; ops stay our cost to manage).
