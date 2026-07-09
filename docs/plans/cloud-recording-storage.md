# Cloud Recording Storage (R2) — Implementation Plan

**Board card:** #64 Cloud recording storage (R2) + GB-month billing (ToDo)
**Goal:** Keep a durable copy of motion-triggered clips in object storage, so footage survives
the edge box being stolen, smashed, or wiped — and bill the storage honestly.

## Why this is the strongest feature available

Today a clip lives only on the edge's local ring (`max_storage_mb`, default 1 GB). If the
burglar takes or destroys the box, the recording of them doing it goes with it. **The
footage is only as safe as the hardware being stolen.** That is the single reason people pay
for Ring/Nest cloud plans. Everything else here is secondary.

## Economics (why it's affordable)

Assumptions: H.264 1080p ≈ 2 Mbps (0.25 MB/s), 30 s clip ≈ 7.5 MB, **motion-triggered**
(what Sentinel already does), 30-day retention.

| | Clips/day | Stored | R2 @ $15/TB | Revenue | % of revenue |
|---|---|---|---|---|---|
| Home (6 cam) | 120 | ~27 GB | $0.41/mo | $7.25 | ~6% |
| Business (16 cam) | 960 | ~216 GB | $3.24/mo | $91 | ~4% |

Margin moves from ~97–99% to ~94–98%. **Storage cost is not the deciding factor.**

**Continuous 24/7 recording does not work at these prices:** 2 Mbps × 24 h = 648 GB per
camera per month → a 16-camera Business tenant is ~10.4 TB/mo = **$155/mo on $91 revenue**.
It is a different product; do not offer it on these tiers.

## Decisions locked

- **R2, not B2.** Zero egress at any volume, no CDN to configure. B2 is $6/TB vs R2's $15/TB,
  but that is ~$2/mo per Business tenant — not worth fronting B2 with Cloudflare.
- **Motion clips only.** Continuous recording is out of scope (see above).
- **The edge never holds R2 credentials.** Control mints presigned URLs.

## Open decisions (decide before building)

1. **Included or metered add-on?** The billing engine already has `included_storage_gb` and
   `overage_storage_per_gb`. Recommend: included with a per-plan cap, overage beyond it.
2. **Retention per plan** — *this is the cost lever*, not the price. Proposal: Beta 3 d /
   Home 14 d / Business 30 d.
3. **GB-month or ingest?** See "The billing change" — they differ, and only one is honest.
4. **Overage rates are currency-blind.** `billing_plans.overage_*` is a single number, while
   prices moved to `plan_prices (plan, currency, amount)` in migration 0044. Either move the
   overage rates there too, or convert with `fx_rates`. Must be settled before a rate ≠ 0.
5. **Keep the local clip after upload?** Recommend yes — the edge ring stays the fast path for
   recent playback; R2 is the durable copy.
6. **LGPD:** who is the controller? What is the deletion SLA?

## Architecture

Direct **edge → R2**, so no video crosses the VPS (it has 75 GB of disk and a shared NIC).

1. Edge finishes a clip and already posts metadata to `POST /config/recordings/clip-metadata`
   (`ClipPush { camera_ip, clip_id, friendly_name, duration_secs, size_bytes, started_at }`,
   handler `sentinel-control/src/http.rs::clip_push`).
2. Control answers with a **presigned PUT URL** + object key. Control holds the R2 key; the
   edge holds nothing. `rust-s3` (already a dependency, 0.35) can presign.
3. Edge uploads directly to R2, then confirms; control sets `recordings.cloud_key`.
4. Playback: control mints a short-lived **presigned GET**; the app streams from R2. Zero VPS
   egress, and R2 charges none.

**Object key encodes retention** — `{days}d/{tenant_id}/{yyyy}/{mm}/{clip_id}.mp4` — so R2
lifecycle rules (one per tier: `3d/`, `14d/`, `30d/`) expire objects with no purge job. A plan
upgrade changes the prefix for *new* clips; existing ones keep the retention they were stored
under. Simple, and it never silently keeps data longer than promised.

We already do exactly this pattern for minio staging: `media::ensure_lifecycle` sets the
bucket rule from code at boot, because the VPS compose is hand-maintained and never receives
the repo's copy.

## Phases

### Phase 1 — Durable copy
- Migration: `recordings` + `cloud_key text NULL`, `cloud_uploaded_at text NULL`,
  `upload_state text NOT NULL DEFAULT 'pending'` (`pending|uploaded|failed`).
- `app_core::media`: presign PUT/GET (a second bucket handle — R2 endpoint, separate from the
  minio staging bucket; they are different stores with different jobs).
- Control: extend `clip_push` to return the presigned PUT + key; add `POST
  /config/recordings/:clip_id/uploaded` for the edge to confirm.
- Edge: an upload queue with retry + backpressure. **Must not starve live view** — cap the
  rate and never upload while a live session is active on that camera.
- R2 bucket + one lifecycle rule per retention tier, set from code at boot.

### Phase 2 — Playback + UI
- `GET /recordings/:id/url` → short-lived presigned GET.
- Recordings page: a badge distinguishing *on the box* from *in the cloud*; play either.
  Reuse the existing empty/loading states.

### Phase 3 — Retention
- `billing_plans + retention_days int`.
- Prefix selection at presign time from the tenant's current plan.
- Cancelled/deleted tenant → purge every object under `*/{tenant_id}/`.

### Phase 4 — **The billing change** (the substantive one)

Storage is a **stock**, not a flow. Today `invoices::usage_for_period` does:

```sql
SELECT SUM(size_bytes) FROM recordings WHERE tenant_id = $1 AND started_at LIKE '2026-07%'
```

Two things are wrong with that as a storage charge:

1. **It counts bytes we don't store.** Those rows describe clips on the *edge's* disk. The
   invoice already prints a "storage — N GB" line for footage that never left the customer's
   own hardware. Harmless only because `overage_storage_per_gb` is `0`; the day a rate is set,
   we start billing people for their own disk.
2. **It sums ingest, not occupancy.** A tenant who uploads 30 GB and keeps it 30 days is
   charged the same as one who keeps it 3 days — but they cost 10× more. Retention, the thing
   we actually pay for, would not appear in the bill at all.

The honest unit is **GB-month**: the average stored bytes over the period.

- New table `tenant_storage_daily (tenant_id, day, bytes)`.
- **pg_cron** samples it nightly (pure SQL, so it belongs there — see migration 0046):
  `SUM(size_bytes) WHERE cloud_key IS NOT NULL AND upload_state = 'uploaded'`.
- `usage_for_period` reads `AVG(bytes)` over the period's days → `storage_gb`.
- The invoice line becomes `max(0, gb_month − included_storage_gb) × overage_storage_per_gb`,
  which is what `compute()` already does — only the *input* changes.

Also fix `usage_for_period` to count only `cloud_key IS NOT NULL`, regardless of which unit
wins. That is a correctness fix worth doing even if cloud recording is never built.

### Phase 5 — LGPD
- Deletion must reach R2, not just the row. A "delete my data" request that leaves objects
  behind is the whole problem.
- Encryption at rest (R2 default), per-tenant key prefix, documented retention justification.

## Out of scope (v1)

- Continuous 24/7 recording (economics above).
- Cross-region replication.
- Client-side encryption (the customer holding the key means we cannot transcode or thumbnail).
- Migrating the minio staging bucket to R2 — it is a hand-off buffer with a 3-day expiry and
  belongs next to the services that use it.
