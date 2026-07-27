# Ship logs to Datadog, off Postgres — Implementation Plan

**Board card:** #145 logs → Datadog (stop logs competing with product data in Postgres)
**Goal:** Get the high-volume, low-value **log/trace** streams out of Postgres and into **Datadog**
(free tier, 1-day retention) so they stop competing with **product + client data** for DB write
throughput, disk, and the nightly retention sweeps — while the **product event stream stays in
Postgres**.

## What moves vs. what stays

| Data | Table / module | Decision |
|---|---|---|
| **Platform debug trace** | `platform_logs` (migration 0028) · `app_core::platform_logs::LogQueue` — every subsystem pushes; a drain batch-inserts every 500 ms | **→ Datadog** |
| **Edge log relay** | `box_logs` (migration 0016) — the edge POSTs lines to `/config/logs` (`box_logs_push`), control persists + re-broadcasts on `log_bus` | **→ Datadog** (keep the live `log_bus` SSE tail) |
| **Product event stream** | `events` (0052/0053/0054) · `app_core::events::EventQueue` — alerts, presence, clips, auth | **stays in Postgres** (it *is* the product; the Playback/Timeline + notifications read it) |
| **Async job queue, run history, client/tenant data** | `work_jobs`, `workflow_runs`, tenants/devices/… | **stays in Postgres** |

The insight: `platform_logs` + `box_logs` are append-only firehoses read almost never (only the
Boxes log viewer + debugging), yet they carry the heaviest write + retention load. `events` is
low-volume and product-critical. Splitting them is the whole card.

## Datadog integration — direct HTTP intake (no agent)

A small shipper in `app-core` posts batches to the **Datadog Logs Intake API**
(`https://http-intake.logs.datadoghq.com/api/v2/logs`, `DD-API-KEY` header, gzipped JSON array).
No Datadog Agent to run in the cluster — control/flow already batch these lines in memory, so the
drain just changes its sink from a Postgres `INSERT` to an HTTPS POST. Each record carries
`ddsource=sentinel`, `service` (control/flow/edge), `ddtags=tenant:<id>,box:<id>,pod:<name>`, and
the level/target/message — so Datadog's facets replace the SQL `WHERE` filters we have today.

## Phase 1 — the shipper (`app-core`)
- New `app_core::datadog` (mirrors `platform_logs::LogQueue`'s spawn/push/drain shape): an in-memory
  channel + a drain task that batches (size/time) and POSTs to the intake API; drops on backpressure
  (a lost debug line is acceptable; never block a request). `DD_API_KEY`/`DD_SITE` from env; **unset →
  no-op** (dev/local keeps working, logs just aren't shipped).

## Phase 2 — platform_logs → Datadog
- Point `platform_logs::LogQueue`'s drain at the Datadog shipper instead of the `platform_logs`
  INSERT (or fan to both behind a flag for one release, then drop the INSERT).
- The platform-log **viewer** (if any UI reads `platform_logs`) either queries Datadog or is dropped.

## Phase 3 — box_logs → Datadog + remove the in-app logs view
- `box_logs_push` (`/config/logs`, control `http.rs`): **keep the endpoint** (the edge keeps POSTing
  its lines — it holds no Datadog key), but change the sink to the Datadog shipper instead of
  `st.log_bus.send(...)` + `insert_box_logs(...)`.
- **Remove the in-app logs view entirely** — logs live in Datadog now, and home customers never need
  a log screen (it's an operator/debug concern). Delete:
  - Frontend: `sentinel-app/src/routes/logs/+page.svelte`, and the per-box **"View logs"** button +
    `ScrollText` import in `settings/boxes/+page.svelte`.
  - Control: `GET /boxes/:id/logs` (`box_logs`, history) and `GET /boxes/:id/logs/stream`
    (`box_logs_stream`, the `log_bus` SSE relay) routes + handlers; the `log_bus`
    (`broadcast::Sender`) in `AppState`; `store::list_box_logs` / `insert_box_logs`.
- Operators read logs in **Datadog** (facets: `box:<id>`, `tenant:<id>`, `service:edge`).

## Phase 4 — retire the tables — DONE (partial, operator call)
- `box_logs` **dropped** (migration `0072_drop_box_logs.sql`). It was a pure append-only firehose with
  no reader/writer left since Phase 3 (no pg_cron sweep — pruned on ingest, and that code is gone), so
  the drop reclaims its write + vacuum load with nothing to break. Validated against the live DB
  (10,058 stale rows) before deploy.
- `platform_logs` **kept** (operator decision): the workflow editor's per-run log panel still reads it
  (`GET /platform-logs`, filtered by `trace_id`). Note it's frozen in prod since Phase 2 (flow ships to
  Datadog, not PG), so that panel shows only pre-#145 rows — retiring it (remove the panel, or proxy
  the query to the Datadog Logs API) is a separate future decision, tracked outside this card.

## Open decisions
1. ~~Boxes log-history view~~ — **RESOLVED (operator call): remove the in-app logs view entirely;
   Datadog is the only log viewer.** No in-app history/stream, so no Datadog app-key proxy to build.
2. **Dual-write window** — fan platform/box logs to *both* Postgres and Datadog for one release to
   de-risk, or hard-switch. (Lean hard-switch behind the `DD_API_KEY`-set gate.)
3. **Free-tier volume** — Datadog free logs cap; confirm the edge relay + platform trace fit, or
   sample/level-filter (e.g. ship `info`+ from the edge, `debug` only from control) to stay under.

## Out of scope (v1)
- Moving the **product event stream** (`events`), run history, or any client/tenant data — those stay
  in Postgres.
- Datadog **metrics** (the `X-Edge-Metrics` heartbeat / resource gauges) and **APM/traces** — logs only.
- A self-hosted log store (Loki/ELK) — the ask is Datadog's free tier for now.
