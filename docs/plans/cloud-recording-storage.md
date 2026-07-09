# Cloud Recording Storage (R2) — Implementation Plan

**Board card:** #64 Cloud recording storage (R2) + GB-month billing (ToDo)
**Goal:** Keep a durable copy of motion-triggered clips in object storage, so footage survives
the edge box being stolen, smashed, or wiped — and bill the storage honestly.

## Why this is the strongest feature available

A clip lives only on the edge's local ring (`max_storage_mb`, default 1 GB). If the burglar
takes or destroys the box, the recording of them doing it goes with it. **The footage is only
as safe as the hardware being stolen.** That is the one reason people pay for Ring/Nest cloud
plans. Everything else here is secondary.

## Decisions locked (2026-07-09)

**Retention is the product. The quota is a guardrail.** A customer buys *time* ("30 days of
cloud recording"), not gigabytes — nobody can reason about GB. But the edge can be made to
record **continuously via a workflow**, and without a cap that tenant becomes a silent loss.
So the plan sells days; the GB quota exists only so the outliers pay.

| Plan | Cameras | Retention | Included | Normal use | % of quota |
|---|---|---|---|---|---|
| Beta | 2 | **3 d** | 5 GB | 0.4 GB | 9% |
| Home | 6 | **30 d** | 100 GB | 26 GB | 26% |
| Business | 16 | **60 d** | 750 GB | 422 GB | 56% |

Overage: **R$ 0,30 / US$ 0,06 per GB-month** (≈ 3.9× our R2 cost of R$ 0,077/GB-month).
Nobody on normal motion-triggered use approaches the quota. A 16-camera Business tenant
recording continuously stores ~19.8 TB, costs us ~R$ 1.567, and is billed ~R$ 5.850 of
overage — margin holds at ~3.7×.

- **Billing unit: GB-month.** Average bytes stored over the period, not bytes ingested.
- **R2, not B2.** Zero egress at any volume, no CDN to configure. B2 is $6/TB vs R2's $15/TB,
  but that is ~$2/mo per Business tenant — not worth fronting B2 with Cloudflare.
- **Motion clips only.** Continuous recording is not a plan feature. It remains *possible*
  through a workflow on the edge, and the overage is what makes that honest.
- **The edge never holds R2 credentials.** Control mints presigned URLs.
- **Keep the local clip after upload.** The edge ring stays the fast path for recent playback;
  R2 is the durable copy.
- **Money lives in `plan_prices`, quotas live in `billing_plans`.** This settles the
  currency-blind overage rates: a GB is a GB in any currency, but a *rate* is money. Migration
  0044 already moved prices to `plan_prices (plan, currency, amount)`; the overage rates join
  them there, and `billing_plans` keeps `included_*` and the new `retention_days`.

### LGPD
The **customer is the controller**; we are the operator/processor. Consequences:
- The customer can delete their cloud clips themselves, at any time, and **every deletion is
  audited** (`platform_logs`).
- Retention expiry is automatic (R2 lifecycle), and its justification is documented.
- Deletion must reach R2, not just the row — a "delete my data" that leaves objects behind is
  the whole problem.
- Being the processor means we owe the customer a **DPA (contrato de operador)**. Flagged, not
  drafted here.

## Architecture

Direct **edge → R2**: no video crosses the VPS (75 GB of disk, one shared NIC).

1. The edge finishes a clip and already posts metadata to
   `POST /config/recordings/clip-metadata` — `ClipPush { camera_ip, clip_id, friendly_name,
   duration_secs, size_bytes, started_at }`, handled by `sentinel-control/src/http.rs::clip_push`.
2. Control answers with a **presigned PUT URL** + object key. Control holds the R2 key; the
   edge holds nothing. `rust-s3` (already a dependency at 0.35) presigns.
3. The edge uploads directly to R2, then confirms; control sets `recordings.cloud_key`.
4. Playback: control mints a short-lived **presigned GET**; the app streams from R2. Zero VPS
   egress, and R2 charges none.

**The object key encodes retention** — `{days}d/{tenant_id}/{yyyy}/{mm}/{clip_id}.mp4` — so one
R2 lifecycle rule per tier (`3d/`, `30d/`, `60d/`) expires objects with no purge job. A plan
upgrade changes the prefix for *new* clips; existing ones keep the retention they were stored
under, so we never silently keep data longer than promised.

Set the lifecycle **from code at boot**, exactly as `media::ensure_lifecycle` already does for
the minio staging bucket: the VPS compose is hand-maintained and never receives the repo's
copy, so a rule declared there would simply never exist in production.

## Clip state (requirement 10)

`recordings.upload_state`: `pending | uploading | uploaded | failed`. The UI badges read:

| Badge | State | Meaning |
|---|---|---|
| **Local** | `pending` | on the box only — not yet durable |
| **In transit** | `uploading` | being copied to the cloud |
| **Cloud** | `uploaded` | durable; survives the box |
| **Failed** | `failed` | the edge exhausted its retries, or the ticket expired with the clip still in transit |

## Phases

### Phase 1 — Durable copy — **shipped**
- Migration 0047: `recordings + cloud_key, cloud_uploaded_at, upload_state` (+ CHECK).
- `app_core::clip_store`: a **second** bucket handle for R2 (a different store with a different
  job than the minio staging bucket) + `presign_put` / `presign_get` / `delete`.
- Control: `clip_push` returns the presigned PUT + key; `POST /config/recordings/:clip_id/uploaded`
  for the edge to confirm.
- Edge (`clip_upload.rs`): bounded queue, **one upload at a time** — the box serves live video over
  the same uplink. Four attempts over ~6 min, but a 4xx is permanent (an expired presign fails
  identically every time, and the single consumer would make the next clip wait).
- R2 bucket + one lifecycle rule per retention tier, applied at boot and **read back** to arm.

Two things only the build revealed:

- **The PUT must carry `Content-Length`.** Handed an unsized reader, ureq falls back to
  `Transfer-Encoding: chunked`, which S3/R2 reject on a presigned PUT. Setting the header makes
  ureq stream the file straight through, so a clip never sits in memory. `examples/r2_probe.rs`
  exercises exactly this path against the real bucket.
- **Nothing ever flipped `uploading` back.** Control sets it when it mints the ticket and only
  the edge moves it forward, so a dead box / full queue / lost confirm left the row reading
  "In transit" forever. Migration 0048 sweeps rows whose ticket has certainly expired (pg_cron,
  hourly) to `failed`, keeping `cloud_key` so an orphaned object is still findable.

### Phase 2 — Playback + clip state (requirements 9, 10)
- `GET /recordings/:id/url` → short-lived presigned GET. **Cloud clips first** (simplest); the
  existing edge clip endpoint keeps serving local ones.
- Recordings page: the four badges above; play whichever copy exists.

### Phase 3 — Retention
- `billing_plans + retention_days integer NOT NULL DEFAULT 0` (0 = no cloud recording).
- Prefix chosen at presign time from the tenant's current plan.
- Cancelled/deleted tenant → purge every object under `*/{tenant_id}/`.

### Phase 4 — **The billing change** (the substantive one)

Storage is a **stock**, not a flow. `invoices::usage_for_period` currently does:

```sql
SELECT SUM(size_bytes) FROM recordings WHERE tenant_id = $1 AND started_at LIKE '2026-07%'
```

Two things are wrong with that as a storage charge:

1. **It counts bytes we don't store.** Those rows describe clips on the *edge's* disk. The
   invoice already prints a "storage — N GB" line for footage that never left the customer's
   own hardware. Harmless only because `overage_storage_per_gb` is `0`; the day a rate is set,
   we bill people for their own disk. **This is a correctness fix worth making even if cloud
   recording is never built.**
2. **It sums ingest, not occupancy.** A tenant who keeps 30 GB for 60 days is charged the same
   as one who keeps it 3 days — but costs 20× more. Retention, the thing we actually pay for,
   would not appear in the bill at all.

The fix:
- Migration 0047:
  - `billing_plans + retention_days`; seed 3 / 30 / 60.
  - `plan_prices + overage_gpu_per_sec, overage_storage_per_gb, overage_notification_each`
    (money, per currency); **drop** the same three from `billing_plans`.
  - `billing_plans.included_storage_gb` — seed 5 / 100 / 750 (currency-neutral, stays put).
  - `tenant_storage_daily (tenant_id, day date, bytes bigint, PRIMARY KEY (tenant_id, day))`.
- **pg_cron** samples it nightly (pure SQL, so it belongs there — see migration 0046):
  `SUM(size_bytes) WHERE cloud_key IS NOT NULL AND upload_state = 'uploaded'` grouped by tenant.
- `usage_for_period` reads `AVG(bytes)` over the period's sampled days → `storage_gb`.
- `compute()` is unchanged: it already computes
  `max(0, used − included) × overage_rate`. Only the *input* and the *rate's source* change.

### Phase 5 — Storage visibility (requirements 7, 8)
- **Master** (`/admin/billing` or a new `/admin/storage`): every tenant — **including Beta** —
  with stored GB, GB-month for the period, quota, % used, and projected overage. Sorted by
  cost. This is the view that catches a continuous-recording tenant before the invoice does.
- **Tenant** (`/recordings` header or `/billing`): stored GB vs quota, retention in days, and
  a plain sentence about what happens at the cap. No dead end: link to upgrade.

### Phase 6 — LGPD
- Client-initiated delete (single clip + bulk), audited to `platform_logs`.
- Cancel/delete tenant → purge objects.
- Document retention justification; DPA drafted with counsel.

## Out of scope (v1)

- **Continuous 24/7 as a plan feature** (16 cameras ≈ 19.8 TB stored = R$ 1.567/mo of cost
  against R$ 499 of revenue). It stays possible via workflow, priced through overage.
- Cross-region replication.
- Client-side encryption (a customer-held key means we cannot transcode or thumbnail).
- Migrating the minio staging bucket to R2 — it is a hand-off buffer with a 3-day expiry and
  belongs next to the services that use it.
