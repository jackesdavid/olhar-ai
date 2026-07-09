# Tenant Cloud Cost & Billing — Implementation Plan

**Use case:** _(none yet — seeded from a direct request; see "Decisions")_
**Board card:** #63 Tenant Cloud Cost — view + calculate per-tenant spend (ToDo)
**Goal:** Two linked outcomes: **(A) customer-facing invoicing** — each tenant billed a plan base fee + metered overage (GPU, storage, notifications), auditable and immutable per period; and **(B) internal COGS/margin** — what that tenant actually costs us to run, so we can see margin = invoice − cost. (A) is the primary v1 deliverable (decided 2026-07-07).

## Build status — RESUME HERE (2026-07-08)

**Branch:** `feat/tenant-billing` (off `main`). Everything stacked here to send in one go.
Backend `cargo check --workspace` clean; frontend `svelte-check` clean for new files
(the 9 reported errors are pre-existing, elsewhere). **Not yet pushed / not deployed.**

**Commits (newest first):**
- `46e93b4` Phase 6b — **Stripe gateway** (BR + US), generalized provider seam
- `5dee771` Phase 6a — **multi-currency** (BRL + USD)
- `742389f` payment link on subscribe · `b06e8ec` Phase 5b — billing UI
- `1b66b7e` Phase 5a — master plan administration API
- `4b10bff` Phase 4 — Asaas payments + seam (+ `592f688` env wiring)
- `3c56d75` Phase 3 — invoices · `1f424bd` Phase 2 — plans/subs/FX · `d9189ec` Phase 1 — GPU metering
- (also `50cbb82` retired `new-ui` → `main` is the single mainline)

**Done:** Phases 1–6. Migrations **0039–0044**. Pricing: Beta 2cam/30d free · Home
R$39,90 / **$9,99** · Business R$499 / **$149**.

**Gateway decision (2026-07-08):** **Stripe now, Asaas later.** Stripe added recurring
**Pix Automático** in 2026-04, so one integration serves Brazil *and* the American market
(multi-currency Prices). Asaas stays in-repo behind the seam for when its cheaper Pix fees
(0.22–0.35%) / native NF-e matter. `PAYMENT_PROVIDER` defaults to `stripe`.

**Stripe shape (why the trait changed):** Stripe is *Checkout-style* — the Subscription
exists only **after** payment, so `start_subscription()` returns `{payment_url, subscription_ref?,
price_ref?}` and the ref arrives on `checkout.session.completed`. Webhooks are HMAC-signed
over the raw body and **idempotent** (`payment_events` per event id). Consequence: the
tenant moves onto the paid plan **only when payment confirms**, never optimistically (this
changed Asaas too — it now records `pending` first). Our code **auto-creates the Stripe
Product+Price** and persists `plan_provider_price.price_ref` for reuse.

**Env:** `AI_RATE_PER_SEC`, `PAYMENT_PROVIDER=stripe`, `STRIPE_API_KEY`,
`STRIPE_WEBHOOK_SECRET`, `STRIPE_SUCCESS_URL`/`STRIPE_CANCEL_URL` (+ Asaas vars kept).
Documented in `.env.example`, wired in `deploy/vps/docker-compose.sentinel.yml`.
Local `.env` already has a **test** `rk_test_…` key; `STRIPE_WEBHOOK_SECRET` still blank.

**Pending / next:**
1. **`STRIPE_WEBHOOK_SECRET`** — either `stripe listen --forward-to localhost:8790/webhooks/stripe`
   (prints a `whsec_…`, no public URL needed) or add the VPS endpoint
   `https://control.thesentinels.online/webhooks/stripe` and copy its signing secret.
2. **Real end-to-end test on the Default tenant** (runbook below).
3. **Enforcement (Phase 2b, NOT built):** camera-cap block on device-create, self-service
   trial auto-start on signup, expiry soft-lock. Helpers exist (`subscriptions::camera_cap`,
   `expire_due_trials`) but aren't wired into the flows.
4. Push branch + PR to `main` once the test passes.

### Real-test runbook (Stripe, test mode)
1. Stripe dashboard in **Test mode**. The `rk_test_…` key is already in `.env`.
2. Get `whsec_…` (see Pending #1) → `.env` / `sentinel.env`.
3. **Do NOT create Product/Price by hand** — our code creates them on the first subscribe
   (needs Products+Prices *write* on the restricted key; the "Recurring subscriptions and
   billing" template covers it).
4. **Test with card first** (`4242 4242 4242 4242`). Pix/boleto must be enabled on the
   account under Settings → Payment methods.
5. **Perms gotcha:** `billing.read`/`billing.write` are NEW perms; the Default tenant's
   Admin role lacks them → grant via Settings → Roles → Admin, else the nav link won't show.
6. Deploy (`task cloud-deploy`, OrbStack up) or run locally; migrations apply on control boot.
7. As Default Admin: `/billing` → Home → card → Confirm → Stripe Checkout opens → pay →
   `checkout.session.completed` webhook binds the subscription and flips the tenant to
   `active`.
8. Verify: DB `payment_subscriptions`, `payment_events`, `tenant_subscriptions`; master
   `/admin/billing` shows Default = Home/active; "Close" a period → invoice in `/billing`.

**Risk to watch:** the Price is created in the **tenant's currency**. Default tenant is
`BRL` — the Stripe account must accept BRL. If it errors, test with
`UPDATE tenants SET currency='USD'` and the $9,99 Home price.

**Security note:** a `rk_test_…` key was pasted into a chat transcript — roll it after testing.

## Decisions locked (2026-07-07)

- **Goal = customer invoicing + payment collection** (not just internal COGS). ⇒ needs a plan/subscription model on tenants, auditable per-unit metering, immutable period-closed invoices, **and a Brazilian payment gateway (Pix + boleto + cartão) — now in scope (Phase 4)**.
- **AI/GPU metering first.** RunPod GPU-seconds is the dominant, currently-invisible cost — build the `ai_usage` metering before the UI, so no number is fabricated.
- **Both audiences.** Master sees all tenants (cross-tenant); each tenant admin sees their own invoice (needs `billing.read` RBAC + tenant-scoped endpoint).
- **RunPod attribution = self-metered from the envelope.** RunPod has no per-request cost center (cost centers are account/team-level; it bills the whole endpoint by GPU-second). We attribute in-app: every `/runsync`/`/status` reply returns `delayTime` + `executionTime` (the billed ms) and we know the tenant at call time.
- **Shared VPS cost is NOT a customer invoice line.** On the invoice it's absorbed into the **plan base fee**. Measured per-tenant VPS allocation is deferred (Phase 5) — at $6.75/mo infra it's economically pointless; margin is ~99% regardless.
- **Pricing = flat tiers, BRL, aggressive (seed usage).** Costs are pennies, so price on value/market, aggressively, to acquire customers + usage data:
  - **Beta (free trial) — 2 cameras — 30 days — R$ 0** (locked 2026-07-07). **Self-service:** signing up auto-starts the 30-day trial. At day 30 → **soft-lock + prompt** (view becomes read-only, prompt to pick a paid plan; data retained ~7 days before purge), not a hard lockout. Cap = 2 *active* cameras (swappable).
  - **Home — up to 6 cameras — R$ 39,90 / US$ 9,99 per month** (~97% margin)
  - **Business Basic — up to 16 cameras — R$ 499 / US$ 149 per month** (~99% margin)
  - Prices are per-currency rows in `plan_prices`; a tenant bills in `tenants.currency`.
- **Payments = Stripe (superseded Asaas, 2026-07-08), provider-owned subscriptions.** Stripe gained recurring **Pix Automático** (2026-04), so one integration covers **Brazil + the American market** with multi-currency Prices (card, boleto, Pix). Asaas remains implemented behind the seam (`PAYMENT_PROVIDER=asaas`) for when its cheaper Pix fees / native NF-e win. **NF-e deferred** either way. The provider owns the billing cycle + dunning; we mirror charges into `invoices` and gate access from idempotent webhooks.
- **Currency = multi-currency (BRL + USD) since Phase 6a.** Each tier is priced per currency; the tenant's `currency` selects it. FX (`fx_rates`, USD→BRL, floating with last-quote fallback) is kept for internal COGS and any USD-denominated overage. Originally BRL-only — Our costs (Contabo, RunPod) are USD; BRL is the billing/display currency. **Plan base fees are fixed BRL numbers** (marketing stability — R$39,90 / R$499 don't float with the dollar). Floating USD→BRL conversion applies to **internal COGS and any USD-denominated usage overage** (e.g. RunPod GPU). See the FX design in Phase 2. *(Open: confirm base fee stays fixed-BRL vs. whole-plan-floats — recommended fixed.)*

## Context — what exists today

There is **no billing/cost/usage-metering infrastructure** anywhere. `tenants(id, name, slug, status, created_at)` has no plan/subscription/price columns. Most cost drivers are already tenant-scoped in Postgres, so the marginal drivers are largely *aggregation over data we already keep* — except GPU, which needs new instrumentation.

**Countable today (per tenant, existing tables):**
- Box count + latest resource % — `tenant_boxes` (cpu/mem/disk/gpu_pct, `metrics_at`) — migration 0038
- Workflow runs (with `started_at`/`ended_at` → duration) — `workflow_runs` — migration 0015
- Notifications sent (by status) — `notification_queue` — 0035/0036 (index `(tenant_id, status, created_at)`)
- Inbound events — `event_queue` — 0035
- Recording storage bytes — `recordings.size_bytes bigint` — 0030 (no SUM query exists yet)
- Camera count — `streaming_cameras` / `devices`

**Gaps needing new instrumentation:**
- **AI sidecar / RunPod GPU calls** — only `log::info!` in `crates/app-core/src/agent/ai_service.rs`, never persisted. The single most expensive driver, invisible today. **Phase 1.**
- Gateway streaming bytes — `sentinel-gateway` proxy is stateless, records nothing. (Deferred unless egress proves material.)
- Box-metrics history — `tenant_boxes` is overwrite-only; no time-series. (Only needed if COGS ever wants CPU/GPU-hour allocation — Phase 5 optional.)

**Surface / RBAC:** Master console exists — `sentinel-app/src/routes/admin/tenants/+page.svelte`, gated by `auth.me?.is_master`; master routes under `handlers::require_master` in `sentinel-control/src/http.rs`. Master token has **no tenant context**, so cross-tenant queries hit all tenants directly (like `store::list_tenants`), not via `domain::tenant_of()`. RBAC catalog is `sentinel-control/src/rbac.rs` `PERMISSIONS` — **add `billing.read`** for the tenant-scoped self-serve view.

**Next migration number: `0039`** (CLAUDE.md's "0037" is stale — tree already has 0037_tracking_config, 0038_box_resource_usage). Timestamps are RFC3339 `text`, generate with `app_core::alerts::now_rfc3339()`.

## Phase 1 — GPU/AI metering (append-only, auditable)

The dollar-accurate core. Ship this first — it's the cost that dominates and the one that can't be reconstructed after the fact (no history today).

- **Migration `0039_ai_usage.sql`** — append-only `ai_usage(tenant_id uuid, op text, delay_ms int, exec_ms int, success bool, endpoint text, rate_per_sec numeric, cost numeric, created_at text)`. Index `(tenant_id, created_at)`. **Never updated** — immutability is what makes invoices auditable. Store `rate_per_sec` and computed `cost` *at write time* (rate snapshot), so a later rate change doesn't rewrite history.
- **Thread `tenant_id` into `ai_service::call`.** Today `call(op, input)` takes no tenant → change signature to `call(tenant_id, op, input)` and update every call site (detection/enrollment paths in the agent/vision code). This is the biggest mechanical change.
- **Parse billed time from the RunPod envelope** in `resolve_output` — pull `delayTime` + `executionTime` (ms). Local `--rp_serve_api` returns `COMPLETED` synchronously with no timing fields → record `exec_ms = 0`, `cost = 0` (un-metered dev), don't fabricate.
- **Compute** `cost = (delay_ms + exec_ms)/1000 × rate_per_sec`. `rate_per_sec` is configured per endpoint (GPU-dependent: RTX 2060 vs H100) — see the rate model in Phase 2.
- **Gotcha — worker concurrency:** if a worker serves N concurrent requests, summing per-request `executionTime` over-counts vs. actual worker-uptime billing. Our face-recog sidecar is 1-req-per-worker today, so it's accurate; add a note to revisit if concurrency is enabled.
- Storage + notification counters need no new tables — just add the `SUM(recordings.size_bytes)` and `COUNT(notification_queue WHERE status='sent')` queries here so all marginal drivers are queryable per tenant/period.

## Phase 2 — Plan & rate model (subscription)

The pricing scaffolding that turns raw usage into a bill.

- **Migration `0040_billing_plans.sql`**:
  - `billing_plans(id, name, base_fee_brl numeric, camera_cap int, trial_days int NULL, included_gpu_sec, included_storage_gb, included_notifications, overage_gpu_per_sec, overage_storage_per_gb, overage_notification_each, active bool)` — a plan = base fee (fixed BRL) + camera cap + optional trial window + included quotas + overage rates.
  - `tenant_subscriptions(tenant_id, plan_id, started_at text, ended_at text NULL, status text /* trialing | active | expired */)` — tenant ↔ plan over time (a tenant can change plans; keep history for correct past-period invoicing). Current plan = row with `ended_at IS NULL`. For a trial, `ended_at = started_at + plan.trial_days` and `status='trialing'`.
- **Trial + cap enforcement** (the real work beyond the tables):
  - **Camera cap** — block adding a camera past `plan.camera_cap` at the device-create path (control + edge). Beta = 2 active cameras.
  - **Self-service trial start** — signup creates the tenant + a `trialing` Beta subscription (`ended_at = started_at + 30d`) automatically.
  - **Trial expiry** — a check (background sweep or on-request) that flips `trialing → expired` at day 30 and applies the **soft-lock + prompt** (read-only + upgrade prompt; purge data after a grace window). Gate tenant access on `status`.
- **`billing` module** in `crates/app-core/src/billing/mod.rs`:
  - `Plan`, `Subscription` structs mirroring the tables.
  - `TenantUsage { gpu_sec, storage_gb, notifications, box_count, workflow_runs }` — raw per-tenant totals for a period (from Phase 1 queries).
  - `computed_invoice(plan, usage, fx) -> InvoiceDraft` — `base_fee_brl + Σ max(0, used − included) × overage_rate_usd × fx` per driver, with line items.
- Seed three plans (Beta R$0 / Home R$39,90 / Business R$499), so existing tenants aren't unpriced.

**FX / currency (BRL display, USD canonical, floating with fallback):**
- **Migration `0040b_fx_rates.sql`** — append-only `fx_rates(base text, quote text, rate numeric, fetched_at text, source text)`. Latest `(base='USD', quote='BRL')` row = current rate. Append-only so historical invoices can pin the rate that was live at close.
- **Daily fetcher** — a control background task (or pg_cron) pulls USD→BRL from a public FX API and inserts a row. **On fetch failure it inserts nothing → readers fall back to the latest existing row.** Never leaves the system without a quote.
- `latest_fx(base, quote) -> Rate` helper in the `billing` module.
- **Plan base fees are stored as fixed BRL** (don't float). FX converts only USD-denominated pieces: internal COGS (Phase 5) and any USD usage overage (RunPod GPU). Each closed invoice **snapshots the fx rate used** (Phase 3) so it's immutable.

## Phase 3 — Invoice computation + API (immutable period close)

- **Migration `0041_invoices.sql`** — `invoices(id, tenant_id, period text /* YYYY-MM */, plan_id, base_fee, total, status /* draft|final */, created_at)` + `invoice_line_items(invoice_id, driver, qty, unit, included, billable_qty, rate, amount)`. Closing a period snapshots plan + rates into the invoice so it never changes retroactively.
- **`close_period(tenant, period)`** — reads `ai_usage` + storage/notification aggregates for the window, applies the subscription's plan, writes an immutable `final` invoice. Idempotent (re-close replaces only a `draft`, never a `final`).
- **Endpoints** (`sentinel-control`):
  - Master, cross-tenant: `GET /admin/billing?period=YYYY-MM` → all tenants' invoices/drafts; `POST /admin/billing/:tenant/close`.
  - Tenant self-serve: `GET /billing/invoices` + `GET /billing/invoices/:period` — guarded by new `billing.read` perm, scoped via `domain::tenant_of()`.
- **Frontend api**: `billing` object in `sentinel-app/src/lib/api.ts` (`api.get<T>()` pattern), typed `Invoice` / `InvoiceLineItem`.

## Phase 4 — Payments & subscription lifecycle — Asaas (Pix Automático + boleto + cartão)

Turn the computed invoice into actual money. **Provider = Asaas** (locked): BR PSP with recurring **Pix Automático** (launched Jan 2026, ~0.22–0.35%), boleto, cartão recorrente, and native NF-e (turned on later). **Asaas owns the subscription** (billing cycle + dunning); we orchestrate lifecycle + gating.

- **Migration `0042_billing_payments.sql`** — store `asaas_customer_id` + `asaas_subscription_id` on the tenant (+ payment method, current charge status).
- **On trial→paid conversion** (tenant picks Home/Business): create/ensure an Asaas **customer**, then a **subscription** at the plan's fixed BRL price with the chosen billing type (`PIX` / `BOLETO` / `CREDIT_CARD`). The 30-day Beta runs **in-app** (no Asaas subscription during trial); the subscription is created only at conversion.
- **Asaas handles recurring charge, retries/dunning, and payment method** — we don't reimplement billing cycles.
- **Webhook endpoint** in `sentinel-control` — `POST /webhooks/asaas` (verify the Asaas access-token/signature, no JWT). Handle `PAYMENT_CONFIRMED` / `PAYMENT_RECEIVED` / `PAYMENT_OVERDUE` / `PAYMENT_REFUNDED` / `SUBSCRIPTION_*` → update `tenant_subscriptions.status` (`active | past_due | canceled`) → gate access (reuse the trial **soft-lock + prompt** on `past_due`/`canceled`).
- Our **`invoices` table mirrors each Asaas charge** (period, amount, status, `asaas_payment_id`) — auditable history + basis for the margin view. Flat tier invoice = base fee; future metered overage → Asaas one-off charge or its usage API.
- **Config:** `ASAAS_API_KEY` (+ sandbox/prod base URL) in `.env`, read like the other service configs.
- **NF-e / nota fiscal — deferred** (Asaas issues natively; enable in a later step). Not a launch blocker.

## Phase 5 — Billing UI

- **Tenant self-serve** — new route `sentinel-app/src/routes/billing/+page.svelte`: current plan + trial days-remaining, **choose/upgrade plan + pay** (Pix QR / boleto / card via provider checkout), this-period running total, past invoices list + line-item breakdown, payment status. Monochrome ink theme; markup logic in named helpers (house style).
- **Master console** — `sentinel-app/src/routes/admin/billing/+page.svelte`, linked from `+layout.svelte` `is_master` nav block (~line 259): **sees everything** — all tenants × period, trialing tenants + days-remaining, payment/subscription status, total revenue (MRR), per-tenant drill-in, actions: close period, **grant/reset trial (re-trial)**, assume tenant. Reuses the invoice detail component.

## Phase 6 — Internal COGS / margin view — *measured* VPS allocation (DEFERRED — premature at current scale)

**Reality check (2026-07-07): the Contabo VPS is $6.75/mo total** — confirmed **Cloud VPS 10 NVMe: 4 vCPU / 8 GB RAM / 75 GB NVMe**, storage included (Contabo doesn't meter GB separately). → ~10.5M CPU-sec/mo → **~$0.00000064/CPU-sec**, ~$0.0023/CPU-hour, ~$1.69 per vCPU running 24/7. At this scale the whole VPS is pennies: the **Default tenant's 4–5 workflows cost ~$0.02/mo** (ceiling ~$1.69 even at a full vCPU 24/7). A single Home customer at $9.90 covers the entire platform 1.5×; Business at $150 covers it 22×. **Margin is ~99% at any price** — so per-tenant VPS allocation is irrelevant to pricing today. Note: **marginal storage cost is also ~$0** until the 75 GB disk fills (recordings live on the edge; only notification snapshots + pgvector embeddings hit cloud) — at which point the marginal cost is the *next VPS tier*, not $/GB.

**⇒ This phase is deferred until infra actually scales** (multiple VPS, RunPod GPU billed by usage, object storage). Building a measured split to divide $6.75 isn't worth the engineering. When it *does* matter, the design below applies. The metering that matters now is **GPU (Phase 1)** — the only driver that grows with usage and would spike if face-rec moves to RunPod.

Not on the customer's bill — this is for us. When built: replace crude box-count allocation with a *measured* resource split (esp. `sentinel-flow`), so we know the real VPS cost per tenant / camera / workflow.

**Allocation formula:**
```
tenant_vps_cost = vps_monthly
                × (flow_cpu / vps_cpu_total)              -- how much flow weighs on the box
                × (tenant_run_cpu / flow_run_cpu_total)   -- tenant's share of flow compute
```
Drills to **camera** and **workflow** because each `workflow_runs` row has `graph_id` (→ workflow) and the camera comes from the graph↔camera binding.

**Numerator (per tenant/workflow/camera):**
- **Bootstrap, no instrumentation:** wall-clock run-seconds from `workflow_runs` (`ended_at − started_at`) grouped by `tenant_id`/`graph_id`. Available today — enough for a first-cut ranking (e.g. proving the Default tenant's 4–5 workflows cost ~nothing).
- **Accurate:** accumulate active execution time from `run_node_timed` (`crates/app-core/src/workflows/node.rs` already times every node) into a new `workflow_runs.active_ms` column. This is the honest compute number.

**Denominator (total):** flow/control/gateway report their **own container CPU-seconds + mem** (reuse the `edge_metrics::EdgeMetrics` / `X-Edge-Metrics` self-monitoring pattern, pointed at the VPS containers) into a new `service_metrics` time-series table. Gives the real per-service CPU burn.

**Grill / traps to resolve:**
1. **Wall-clock ≠ CPU.** A graph with `Wait`/`Cooldown`/cron gaps has huge wall-clock but ~0 CPU — allocating by run *duration* over-charges idle-but-long graphs. `active_ms` (sum of `run_node_timed`) is the correct signal; wall-clock is only the bootstrap.
2. **Baseline vs marginal.** Much of flow's CPU is scheduler-tick / SSE / idle overhead independent of tenant count. Don't dump 100% of it onto whoever runs workflows — split flow cost into an unattributed **platform baseline** + a **variable** part allocated by run-CPU.
3. **Other services.** Postgres/control/gateway also burn VPS. v1: lump into the platform baseline; meter only `flow` precisely (the thing we want to understand). Attribute control-by-API-calls / gateway-by-stream-seconds later if material.

- **Margin** = invoice total − measured COGS. Surfaced master-only next to the billing dashboard.
- This does **not** change the customer invoice (flat tiers stand); it makes COGS/margin *measured* instead of estimated, and opens the door to a future usage-based tier.

## Phase 7 — Later

- Nightly rollup (pg_cron, like `platform_logs` TTL) into `tenant_usage_daily` so dashboards read pre-aggregated rows.
- Gateway streaming-byte metering, if egress proves material.
- Usage-based / Enterprise tier once GPU metering (Phase 1) shows overage is material.

## Open decisions (grill before building)

1. **Plan structure — one plan or tiers?** Is v1 a single default plan (base fee + overage) for everyone, or multiple tiers (e.g. Home / Business / Enterprise) with different included quotas? Affects seed data + whether the UI needs a plan picker now.
2. **Included quotas — what's "included" vs. overage?** Does the base fee include *some* GPU-seconds / storage GB / notifications and only bill the excess, or is base fee purely the platform seat and *all* usage is metered on top? (Changes the `computed_invoice` math and how bills feel to customers.)
3. **Billing period & close timing.** Calendar month closing on the 1st? Anniversary of signup? Who triggers the close — automatic (pg_cron) or manual master action? v1 could be manual "close period" button + automatic later.
4. **Currency & rounding.** USD only? Round per-line or per-invoice? (RunPod is USD; matters for auditability.)
5. **Retroactivity.** Metering starts empty — the first real invoice can only cover the period after Phase 1 ships. OK to start tenants on a base-fee-only invoice until GPU history accrues?
6. **RunPod `$per_sec` source.** Hand-configured per endpoint (simplest, and RunPod publishes the rate), or pulled from RunPod's billing API? Hand-config recommended for v1.
8. **Payment gateway.** *Locked:* **Asaas**, provider-owned subscriptions, Pix Automático + boleto + cartão, **NF-e deferred** to a later step. No open sub-questions.

7. **Beta trial mechanics.** *Locked:* 2 active cameras, 30 days, self-service auto-start, soft-lock + prompt at expiry, **re-trial can be granted by the master** (a master action that resets a tenant's trial), **master always sees everything** (all tenants incl. trialing + days-remaining). *Still open:* grace window before data purge (7 days?).

## Out of scope (v1)

- **Gateway streaming-byte metering** — deferred unless egress proves material.
- **Box-metrics CPU/GPU-hour allocation** for COGS — flat box-count split is enough for the internal margin view.
- **Dunning, proration mid-period plan changes, taxes** — invoicing mechanics beyond a clean per-period bill.
