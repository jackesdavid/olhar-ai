# Postgres Performance Tuning — from stock defaults to a tuned box — Implementation Plan

**Use case:** inline — "we're on a default Postgres config; research best practices and make it
really fast."
**Board card:** Postgres performance tuning — kill the default config (ToDo, #74)
**Goal:** Replace the out-of-the-box Postgres config with a properly tuned, reproducible,
version-controlled config sized to the VPS, add the observability to keep improving, and fix the
few high-churn / index gaps — measurably faster queries + fewer stalls with zero data-model change.

## Current state (grounding — why this is high-leverage)

- Image: **Postgres 17 + pgvector + pg_cron**, custom `deploy/postgres/Dockerfile` →
  `ghcr.io/jackesdavid/sentinel-postgres:latest`, run in `deploy/vps/docker-compose.sentinel.yml`.
- **The compose `command:` sets ONLY** `-c shared_preload_libraries=pg_cron -c cron.database_name=sentinel`.
  Everything else is **stock default**: `shared_buffers=128MB`, `work_mem=4MB`,
  `effective_cache_size=4GB`, `max_wal_size=1GB`, default planner costs (spinning-disk assumptions),
  no parallelism tuning. On a modern SSD VPS this leaves most of the box's RAM and IO on the table.
- **No `shm_size`** on the container → Docker's default **64MB** POSIX shared memory caps parallel
  workers / large hash + sort ops (a classic Dockerized-Postgres foot-gun).
- **No `deploy: resources`**, no `pg_stat_statements`, no slow-query logging → we're flying blind.
- **Connection pools:** each service opens its own sqlx pool at `max_connections(5)` —
  `crates/app-core/src/db.rs:43`, `sentinel-control/src/main.rs:129` (+ flow, notification). Total
  is modest (~15–25), well under default `max_connections=100`; no pooler. Not the bottleneck yet.
- **Indexes:** reasonable tenant-scoped coverage already (22 indexes) — `events` `(tenant,camera,occurred_at)`
  etc. (`0052`), `work_jobs` claim idx (`0050/0051`), `platform_logs` (`0028`), `recordings`,
  `presence`, `workflow_runs`. So the big win is **config**, not a rewrite — plus a few targeted checks.

## Phase 1 — A tuned `postgresql.conf` (the biggest win)

**First: measure the box.** SSH the VPS (`docs/ref/deploy.md`; `root@85.239.248.86`) → record total
RAM, CPU cores, disk type. Every value below derives from that (PGTune methodology; see refs).

> **Budget cap: Postgres gets ≤ 70% of VPS RAM — NOT the usual 75–80%.** This box is **shared** —
> `sentinel-control`, `sentinel-flow`, `sentinel-gateway`, `sentinel-notification`, `sentinel-redis`
> and MinIO all co-locate on it. Leave **≥ 30% headroom** for the OS page cache + those services so
> nothing gets OOM-killed. Size `shared_buffers` (real allocation) + expected cache so the total
> Postgres footprint stays under 70%, and pin it with the container memory limit in Phase 2.

Author a versioned **`deploy/postgres/postgresql.tuned.conf`** and have the Dockerfile `COPY` it +
`include` it (or reference via `-c config_file=`), keeping the two pg_cron flags. Parameters (fill
from measured RAM `R` / cores `C`; these are the OLTP-web starting points to validate):

- **Memory (stay within the 70% budget):** `shared_buffers ≈ 20–25% R` ·
  `effective_cache_size ≈ 55–65% R` (a planner hint, but keep it inside the 70% cap since the OS
  cache is shared with the other containers) · `maintenance_work_mem = min(5% R, 1GB)` ·
  `work_mem` = conservative (`~ (R*0.2) / (max_connections * max_parallel_workers_per_gather)`,
  start ~16–32MB — it's **per sort/hash node, per query**, so err low). Verify
  `shared_buffers + peak(work_mem × concurrency) + maintenance_work_mem ≤ 70% R`.
- **WAL / checkpoints:** `wal_compression = on` · `min_wal_size = 1GB` · `max_wal_size = 4–8GB` ·
  `checkpoint_completion_target = 0.9` · `checkpoint_timeout = 15–30min` · `wal_buffers = 16MB`.
  (Spreads checkpoint IO, cuts full-page-write churn — fewer periodic stalls.)
- **SSD planner:** `random_page_cost = 1.1` · `effective_io_concurrency = 200` ·
  `default_statistics_target = 100–200` (more for skewed columns).
- **Parallelism (size to `C`):** `max_worker_processes`, `max_parallel_workers`,
  `max_parallel_workers_per_gather` (~C/2), `max_parallel_maintenance_workers`.
- **Autovacuum baseline:** `autovacuum_max_workers = 3–4`, `autovacuum_naptime = 15–30s` (per-table
  overrides in Phase 3).

Apply, **restart `sentinel-postgres` before control** (migration ordering note), verify with `SHOW`.

## Phase 2 — Container / OS knobs

- **`shm_size`** on the postgres service ≈ `shared_buffers` (e.g. `256m`–`1g`) — the single sharpest
  Docker fix; unblocks parallel workers + big hash/sort.
- **`deploy: resources` memory limit ≈ 70% of VPS RAM** on the postgres container (enforces the
  shared-box budget above; keeps the OS + co-located services safe from the OOM killer).
- Consider `huge_pages` (needs host `vm.nr_hugepages` sysctl; document the caveat, likely v2).
- Confirm the image ships a sane malloc (glibc is fine; note jemalloc as a future option).

## Phase 3 — Autovacuum + retention for the high-churn tables

The hot-write tables are `work_jobs` (queue: insert/claim/delete), `platform_logs`, `events`,
`workflow_run_logs`. Defaults let them bloat.

- Per-table `ALTER TABLE … SET (autovacuum_vacuum_scale_factor=0.02, autovacuum_analyze_scale_factor=0.02,
  autovacuum_vacuum_cost_limit=…)` so they vacuum on churn, not on a % of a growing table.
- **Verify the pg_cron TTLs actually run and keep tables small** — `platform_logs` 30-day (`0028`),
  `events` per-plan retention (card #66 Phase 5), clip/job cleanup — check `cron.job_run_details`.
  (This image exists specifically because pg_cron was silently a no-op before — re-verify.)
- One-off bloat check (`pgstattuple` / `pg_stat_user_tables` dead tuples); reindex if needed.
- Consider **time-partitioning** `platform_logs` / `events` if they get large (drop-partition beats
  DELETE for retention) — likely v2, note it.

## Phase 4 — Observability → targeted index/query fixes

- Add **`pg_stat_statements`** to `shared_preload_libraries` (alongside pg_cron) + `CREATE EXTENSION`;
  set `log_min_duration_statement = 200ms` (find slow queries); optional `auto_explain`.
- With real data, `EXPLAIN (ANALYZE, BUFFERS)` the hot paths and fix what's slow:
  - `events` time-travel `(tenant, camera, occurred_at DESC)` (the #67 driver).
  - `work_jobs` atomic claim (`… FOR UPDATE SKIP LOCKED`) — confirm the partial claim index is used.
  - `platform_logs` / `events` feed pagination; `recordings` list.
  - **pgvector face search:** confirm the embedding column has an **HNSW** (or tuned IVFFlat) index —
    a missing/naïve ANN index is the most likely single slow query. Check + add
    (`vision`/`devices` pgvector, memory [[devices-postgres-migration]]).
- Add only the indexes the evidence demands (don't speculate — the baseline coverage is decent).

## Phase 5 — Connection-pool review (defer PgBouncer unless needed)

- Right-size each service's sqlx `max_connections` vs. `max_connections` in the conf; today's ~15–25
  is fine. **PgBouncer** (transaction pooling) only when pods scale — and note the sqlx caveat:
  transaction pooling breaks server-side prepared statements, so it needs
  `statement_cache_capacity(0)` / simple protocol. Document; don't add yet.

## Open decisions (grill before building)

1. **VPS specs** — need exact RAM/CPU/disk before setting values (Phase 1 blocks on it).
2. **Config delivery:** baked-in `postgresql.tuned.conf` via Dockerfile COPY+include (reproducible,
   in git — **recommended**) vs. a long compose `-c` list vs. `ALTER SYSTEM` (persists in the data
   volume, invisible to git). Pick the git-tracked file.
3. **Resource limits:** set a hard memory limit on the container, or leave unbounded on a dedicated box?
4. **pgvector index** type/params (HNSW vs IVFFlat, `m`/`ef_construction`) — confirm current, decide target.
5. **Partitioning** `platform_logs`/`events` now or later.

## Out of scope (v1)

- Read replicas / HA / sharding.
- Major-version upgrade, storage/filesystem changes.
- Table partitioning (flagged as a Phase-3/v2 follow-up).
- PgBouncer rollout (reviewed, deferred).

## Testing / verification

- **Baseline first:** capture `pg_stat_statements` top-N, a handful of `EXPLAIN (ANALYZE, BUFFERS)`
  timings on hot paths, checkpoint frequency, cache hit ratio. Apply tuning. **Re-measure the same
  set** → report before/after. No app code change, so risk is config-only + a Postgres restart
  (deploy postgres before control).

## References (best practices)

PGTune methodology + official wiki; Docker `shm_size` = shared_buffers; WAL/checkpoint + memory guidance:
- [PostgreSQL wiki — Tuning Your PostgreSQL Server](https://wiki.postgresql.org/wiki/Tuning_Your_PostgreSQL_Server)
- [Crunchy Data — Optimize PostgreSQL Server Performance Through Configuration](https://www.crunchydata.com/blog/optimize-postgresql-server-performance)
- [Mydbops — PostgreSQL Parameter Tuning Best Practices 2025](https://www.mydbops.com/blog/postgresql-parameter-tuning-best-practices)
- [Tom Foster — Optimising PostgreSQL Memory Configuration](https://tomfos.tr/postgres/tuning/memory/)
- [Bun — Tuning PostgreSQL performance (most important settings)](https://bun.uptrace.dev/postgres/performance-tuning.html)
