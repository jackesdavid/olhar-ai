# Notification Queue + `sentinel-notification` service — Implementation Plan

**Board card:** #57 Notification queue + sentinel-notification service (ToDo)
**Prerequisite for:** [[telegram-bot-two-way-research]] (#15 — its notify/reply nodes enqueue here)
**Goal:** Extract all notification responsibility into a dedicated `sentinel-notification`
microservice fed by a **durable Postgres queue**. Flow's notify nodes only **enqueue**; the
service owns recipient config (REST), the pairing long-poll loop, and the actual sending, and
pulls media from a shared **object store**.

## Context
Sending currently lives inside the flow pod: `LocalNotifier` (`crates/app-core/src/ports.rs:60,146`)
calls the Telegram client directly, and per-tenant pairing threads run in flow
(`sentinel-flow/src/pairing.rs`, boot `main.rs:54`). That couples flow to provider concerns, blocks
a graph run on send latency, and has no retry/durability (a crash drops the send). Extracting to a
queue-fed service makes sends decoupled, retryable, and independently scalable — and gives #15's
Telegram bot a clean home.

**Confirmed decisions:** durable **Postgres queue** (not lossy Redis pub/sub); media via a shared
**object store (minio, S3-compatible)** — net-new infra (nothing like it exists today).

## Architecture
```
flow (RemoteNotifier)                   sentinel-notification (:8791)
  notify_recipient / deliver_alert  ──▶   claim next due row (SELECT … FOR UPDATE SKIP LOCKED)
    upload media → object store             resolve recipient config (exclusive owner of tables)
    INSERT notification_queue(row, keys)     download media ← object store
    (optional Redis 'ping' for wakeup)       send Telegram: sendMessage / sendPhoto / sendVideo
                                             mark sent | attempts+backoff | dead-letter
  control ──(NOTIFY_URL REST proxy)──▶    config REST: beacons, telegram, provider
                                             per-tenant pairing long-poll loop (moved from flow)
```
The cheap seam: the **`Notifier` trait** (`ports.rs:60`). Swapping `LocalNotifier`→`RemoteNotifier`
leaves every call-site unchanged (`workflows/node/recipients.rs:25`, `alerts/mod.rs:206`,
`agent/tools.rs:537`).

## Phase 1 — New crate + boot + queue table
- New workspace member `sentinel-notification/` (root `Cargo.toml:6`). Keep pure-Rust so the shared
  root `Dockerfile` `PACKAGE=` build cooks it — do NOT `exclude` like `sentinel-edge`.
- `main.rs` modeled on control (`sentinel-control/src/main.rs:107`): sync `Db::connect` (runs
  migrations) → tokio runtime → `http::router` with `/health` → bind `NOTIFY_BIND=0.0.0.0:8791`.
  Copy `shutdown_signal()`.
- Migration `0035_notification_queue.sql`: `notification_queue(id uuid pk, tenant_id uuid,
  created_at text, status text default 'queued' [queued|sending|sent|failed], attempts int
  default 0, next_attempt_at text, target text null, message text, image_key text null,
  video_key text null, result text null)`; index `(status, next_attempt_at)`.

## Phase 2 — Object store (minio) + media plumbing
- Add `minio` to `compose.yaml` + `deploy/vps/docker-compose.sentinel.yml`; env
  `S3_ENDPOINT/S3_BUCKET/S3_KEY/S3_SECRET`.
- Shared client `crates/app-core/src/media.rs` (`rust-s3` or `object_store`): `put(bytes,mime)->key`,
  `get(key)->bytes`, + TTL/expiry for transient notification media.

## Phase 3 — Enqueue seam in flow (`RemoteNotifier`)
- Implement the `Notifier` trait to (a) upload media — snapshot `Image` bytes → `image_key`; video:
  fetch `{edge}/recording/{clip_id}` reusing `resolve_edge_api_base` (`node/recording.rs:92`) →
  upload → `video_key` — and (b) INSERT a `notification_queue` row (+ optional Redis ping). Set as
  the flow `ToolContext` default (`agent/tools.rs:141`).
- `deliver_alert`'s direct `notification::dispatch` (`alerts/mod.rs:206`) routes through the notifier.

## Phase 4 — The service: consume + send + retry
- Queue loop (model on `sentinel-flow/src/bus.rs` injector): claim due rows, resolve config (the
  service is the exclusive reader/writer of `notification_targets`/`telegram_config`/
  `telegram_global_config`), download media, send via the Telegram client (`notification/client.rs`
  send_message/send_photo + **new `send_video`**), mark `sent` or backoff→`failed`.
- Move the per-tenant **pairing long-poll loop** here (from `sentinel-flow/src/pairing.rs` +
  `crates/app-core/src/notification/handler.rs`), reusing `load_all_telegram`; flow no longer polls.

### Inbound events → workflows (the #15 seam) — same durable-queue mechanism, flow as consumer
The long-poll loop no longer lives in flow, so it has no engine handle. Rather than the lossy Redis
`flow:inject` pub/sub, **reuse the exact durable-queue pattern in the opposite direction**: a
Postgres **`event_queue`** (`id uuid, tenant_id, event text, payload jsonb, status
[queued|claimed|done], created_at, …`). Notification INSERTs an inbound event; **flow runs a claim
loop** (`SELECT … FOR UPDATE SKIP LOCKED`) → `engine::fire_event` → mark done. Low-latency wakeup via
Postgres **`LISTEN/NOTIFY`** (no Redis needed), and it survives a flow restart.
```
inbound:  Telegram → notification (reverse-lookup owner tenant, build payload)
                     → INSERT event_queue{tenant,"telegram_message",payload}
          flow claim loop → engine::fire_event → OnEvent("telegram_message") graphs
egress:   graph's Reply to sender → INSERT notification_queue → notification sends
```
One mechanism, two queues (outbound `notification_queue` consumed by notification; inbound
`event_queue` consumed by flow). This makes **inbound events durable** too. The existing motion /
`edge:full-disk` `flow:inject` events can be migrated onto `event_queue` for the same reliability
win (Redis `events:<tenant>` stays pub/sub — that's ephemeral run-progress UI fan-out). Migrating
the motion path is a slightly larger blast radius, so it can be staged.

## Phase 5 — Config ownership — DECIDED: keep control direct-DB (no proxy) ✅
**Decision (2026-07-01):** do NOT build the config REST proxy. Control keeps reading/writing
notification config straight from the shared Postgres — the config UI stays resilient (works even
if the notification service is down), no proxy hop, no new failure mode. The notification service
owns **sending + pairing only**; it already reads the same config tables directly (consumer
`resolve_config` + pairing `load_all_telegram`), so no config REST is needed. Original proxy plan
(service exposes recipient/provider REST, control proxies to `NOTIFY_URL` mirroring `flow_base()`)
is intentionally descoped.

## Phase 6 — Deploy wiring
- `deploy/push-local.sh` `ALL` += `sentinel-notification` (default `dockerfile_for`/`buildarg_for`
  cover it). Compose: new internal service (like flow) with `DATABASE_URL/REDIS_URL/S3_*/NOTIFY_BIND`
  + the `minio` service; control gets `NOTIFY_URL`. Taskfile `notification:` run task. CI
  `deploy.yml` paths+matrix+list (GH Actions billing-blocked — real deploy via `task cloud-deploy`).

## Phase 7 — Notification observability (read-only, tenant-scoped, secondary)
A "no black box" view so we can see what the queues are doing. **All read-only `SELECT` aggregates —
never touches the claim/send path** (the consumer uses `FOR UPDATE SKIP LOCKED`; observability only
reads), so it can't conflict with or slow the queue. Every query is `WHERE tenant_id = $tenant` — a
user sees only their own tenant (tenant rides the JWT; control injects it when proxying,
`sentinel-app/src/lib/api.ts:9-16`).

- **Service endpoints** (on `sentinel-notification`, control-proxied under `beacons.read`):
  - `GET /notifications/stats` → dashboard summary across BOTH queues: pending (queued+sending),
    sent-today, failed-today, inbound-events-today, and throughput = processed-per-hour for the
    current day (`GROUP BY date_trunc('hour', …)`). Tenant-scoped.
  - `GET /notifications/queue?status=&q=&limit=&offset=&sort=&dir=` → paginated + searchable list of
    `notification_queue` rows (status, target, message excerpt, attempts, created_at, sent_at,
    result); returns `{ total, items }`. Search on message/target, filter by status.
- **Retention tie-in**: the queue GC must keep processed (sent/failed) rows ≥ the current day
  (configurable, e.g. 7d) so stats/throughput have data; media objects TTL sooner. Add read index
  `(tenant_id, status, created_at)`.
- **Frontend** (reuse existing patterns, no new libraries):
  - Nav: add `{ label: "Notifications", href: "/beacons/notifications", icon: Bell }` to the flat
    `navItems` (`sentinel-app/src/lib/nav.ts:21-35`; `Bell` already imported).
  - `src/routes/beacons/notifications/+page.svelte` — **dashboard**: KPI stat tiles (Pending, Sent
    today, Failed today, Inbound events today) reusing the Dashboard `stats` grid
    (`routes/+page.svelte:27-32,71-95`); a **hand-rolled inline-SVG bar chart** for throughput/hour
    (no chart lib exists — use `--color-chart-1` / emerald `#34d399`).
  - `src/routes/beacons/notifications/queue/+page.svelte` — **details table**: the Recordings
    backend-search + offset-pagination + sortable-header pattern
    (`routes/recordings/+page.svelte:35-137,351-566`) over queue rows.
  - `$lib/api.ts`: add a `notifications` group (`stats()`, `queue(opts)`) via the `URLSearchParams`
    + `api.get<Page>` idiom (`api.ts:200-212`); hits control (`API_BASE`), tenant-scoped via JWT.

## Tables — stay in shared Postgres
`notification_targets`/`telegram_config`/`telegram_global_config` FK into `tenants`; keep them in
the shared Postgres with `sentinel-notification` as the exclusive writer (no separate DB).

## Out of scope (v1)
- Non-Telegram channels (email/SMS/push) — the queue + service are channel-agnostic; add later.
- #15's inbound bot brain (this card relocates *sending* + adds the queue; #15 builds on it).
- Object-store GC beyond a simple TTL.

## Verification (when implemented)
- Local: `task db-up` + minio; run flow + notification; a `notify_recipient` graph → row in
  `notification_queue` → service claims + sends → `sent`. Kill the service mid-flight → on restart
  the row retries (durability proof). Image path: object key set → downloaded → sent. clip_id path:
  fetched from edge → uploaded → sent as video. Config REST via control proxy lists beacons.
- Observability: the `/beacons/notifications` dashboard shows pending / sent-today / failed-today +
  a throughput bar; the details table lists + searches + paginates queue rows. Sign in as two
  tenants → each sees only its own rows (tenant isolation). Under a send load, confirm the read
  queries neither block nor slow the claim loop (they are plain SELECTs).
- Prod: deploy control+flow+notification+minio via `task cloud-deploy`; smoke a test send from the
  Recipients page.
