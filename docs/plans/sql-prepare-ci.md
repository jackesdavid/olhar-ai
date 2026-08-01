# PREPARE every sqlx statement in CI — Implementation Plan

**Weakness:** [[architecture-weaknesses]] §2 **Board card:** #157 PREPARE every sqlx statement in CI
**Goal:** make placeholder/bind drift a build failure instead of a production outage.

## Why

320 `sqlx::query*` call sites, none compile-checked (app-core is built without the `query!` macro
feature — deliberately, so the build needs no live DB). No test connects to a database, so none are
covered by the 296 tests either.

On 2026-08-01 a removed bind in an `UPDATE` shifted every later placeholder one argument right.
`gstreamer_ok` (boolean) received `platform` (text), Postgres refused the COALESCE, and **every box
token lookup failed** — the edge could not authenticate. `cargo check` passed. Every test passed.

## What the check must catch

Two distinct failures, and `PREPARE` alone only catches the first:

1. **The statement is wrong against the schema** — renamed/dropped column, type mismatch, bad SQL.
   `PREPARE` catches this.
2. **The Rust side disagrees with the statement** — N `.bind()` calls for M placeholders. Postgres
   never sees this; it is the actual shape of the outage. Caught by comparing the site's `.bind()`
   count to the statement's parameter count.

## What the codebase allows (scoped 2026-08-01)

- **Zero** queries are built with `format!` or string accumulation — 308 of 320 sites pass a string
  literal, so they can be extracted statically.
- 12 sites pass a variable (`sqlx::query_as::<_, Device>(&sql)` — a base SELECT plus conditional
  filters) and 7 use `sqlx::QueryBuilder` (`events.rs`, `platform_logs.rs`, `workflows/repo.rs`).
  Both are out of reach of static extraction and are **reported, not silently dropped**.
- Migrations need `pg_cron`, `vector`, `citext`, `pgcrypto`, so stock Postgres cannot run them. The
  test DB must be the `sentinel-postgres` image — which CI now builds (#156), so it is pullable as
  a service image.

## Phase 1 — the test

`crates/app-core/tests/sql_prepare.rs`, skipped unless `TEST_DATABASE_URL` is set, so `cargo test`
stays DB-free for everyone else (the same property that keeps `cargo check` DB-free).

1. Connect, run `sqlx::migrate!("./migrations")` — the schema under test is the migrations, not a
   dump that can drift from them.
2. Walk the workspace crates (`crates/`, `sentinel-control/`, `sentinel-flow/`, `sentinel-gateway/`,
   `sentinel-notification/`; **not** `sentinel-edge/`, which has no DB, nor `legacy/`) and scan for
   `sqlx::query`, `query_as`, `query_scalar` call sites, capturing the SQL literal and the number of
   `.bind(` calls in the chain that follows.
3. For each statement: `PREPARE`, then read `parameter_types` from `pg_prepared_statements` and
   assert it matches the site's bind count.
4. Report every failure in one run with `file:line` — not the first one — and fail the test.

Statements `PREPARE` does not accept (`LISTEN`, `SET`, `TRUNCATE`, DDL) are skipped by leading
keyword. Sites whose bind chain does not parse as a straight-line chain keep the `PREPARE` check and
skip the bind-count check.

## Phase 2 — the CI job

`check:sql` on the `check` stage of `.gitlab-ci.yml`, next to `check:rust`, on merge requests and
`main`. It runs `$CI_REGISTRY_IMAGE/sentinel-postgres:latest` as a service with
`shared_preload_libraries=pg_cron`, and `cargo test -p app-core --test sql_prepare`.

app-core's gstreamer dep is optional and off by default, so this job needs no apt packages — unlike
`check:rust`.

## Decided by the first run (2026-08-01)

**299 statements extracted, 297 prepared, 297 bind-count checked, 0 failures.** The tree is clean
today, so both open decisions resolved themselves:

1. **Parameters Postgres cannot type** — zero occurred. No casts, no allowlist. The two statements
   `PREPARE` will not take are `LISTEN`/`SET`-shaped and are reported as skipped.
2. **Blocking or reporting** — it lands clean, so `check:sql` **blocks** from day one. No
   `allow_failure`, no backlog to grandfather in.

### The first version passed while checking nothing

Worth recording, because it is the weakness this card exists to fix, reproduced inside the fix. The
bind-chain parser stopped at the `)` closing `query(…)`, so all 298 comparisons were skipped — and
the test reported `ok`. Two floors now make that state a failure: `checked > 250`, and
`bind_checked` must exceed three quarters of `checked`.

Fault injection confirms it catches both shapes. Dropping one `.bind()` from
`SELECT pg_notify($1, $2)` and renaming a column in an `events` UPDATE produced:

```
PREPARE failed  crates/app-core/src/events.rs:302
    column "dataa" of relation "events" does not exist
bind mismatch   crates/app-core/src/db.rs:125
    statement takes 2 parameter(s), the call site binds 1
```

## Out of scope (v1)

- Type-checking each `.bind()` argument against `parameter_types`. Needs the Rust type at the call
  site — that is what the `query!` macro is for, and adopting it means a live DB at build time.
- The 12 dynamic-SQL sites and the 7 `QueryBuilder` sites. Covering them needs runtime capture from
  an exercised code path, not static extraction.
- Any statement in `sentinel-edge` (it has no database).
