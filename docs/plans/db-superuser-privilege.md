# The services connect to Postgres as a superuser — Implementation Plan

**Board card:** #173 Services connect to Postgres as a superuser (RCE reach on the DB host)
**Goal:** Give each service the least privilege it needs, so a compromised container or an injection
cannot execute commands on the database host.

## What was found

Verified on production, 2026-08-05, while investigating #160:

```
SELECT rolsuper, rolbypassrls FROM pg_roles WHERE rolname = current_user;
-- superuser = true   bypassrls = true
```

`sentinel-control`, `sentinel-flow`, `sentinel-gateway` and `sentinel-notification` all connect with
the same `DATABASE_URL`, as the role `sentinel`, which is a **Postgres superuser** and additionally
carries `BYPASSRLS`. It also owns all 62 tables in `public`.

## Why this is worth its own card

**A superuser can execute commands on the database host.** `COPY … TO PROGRAM` runs a shell command
as the postgres OS user; a superuser can also `CREATE EXTENSION`, read arbitrary files with
`pg_read_file`, and disable auditing. So the blast radius of *any* SQL injection, or of one
compromised service container, is not "read the database" — it is "run code on the database server".

The queries are runtime strings assembled in 330 places (`check:sql` proves they PREPARE and that
bind counts match, not that no interpolation exists), so "we have no injection" is a claim nobody
can currently evidence.

Two aggravating details:

- **All four services share one role.** The gateway, which serves public media traffic, has exactly
  the same database power as control.
- **`BYPASSRLS` is set**, which is why #160's row-level security cannot work at all until this
  changes. That card is blocked on this one.

## What it is not

Not an active incident, and not evidence of a breach. It is a privilege-escalation *reach* problem:
it makes an ordinary bug catastrophic instead of contained. Prioritise accordingly — above cosmetic
work, below anything currently broken for customers.

## What the experiments showed

Run against the local `sentinel-postgres` on 2026-08-05, because the first version of this plan
assumed things that turned out to be wrong.

**1. Dropping superuser is enough. Ownership does not have to move.**
A role that is `NOSUPERUSER NOBYPASSRLS` but still *owns* the table enforces the policy correctly
once the table is `FORCE`d: an unqualified `SELECT` returned one tenant's row, and an unqualified
`UPDATE … SET note='HACKED'` left the other tenant's row intact.

That removes the four-role split this plan originally proposed. The application can keep owning its
schema and keep running `sqlx::migrate!` on boot. **The security fix is the role attributes, not the
ownership model.**

**2. `CREATE EXTENSION IF NOT EXISTS` is a no-op for a non-superuser — when the extension exists.**
Migrations run as a non-superuser against a fresh database:
- no extensions present → fails at migration 1: `permission denied to create extension "vector" …
  Must be superuser`
- `vector`, `citext`, `pgcrypto` pre-installed by a superuser → sails past them, reaches migration 46
- `CREATE EXTENSION IF NOT EXISTS pg_cron` as a non-superuser where pg_cron already exists →
  `NOTICE: extension "pg_cron" already exists, skipping`, no error

Production has all four installed, so migrations keep working there. **A fresh database does not**,
which makes a superuser bootstrap step a hard requirement for any new deployment or restore.

**3. `cron` needs one grant.** `has_function_privilege('cron.schedule(text,text)','EXECUTE')` is
already true, but `has_schema_privilege('cron','USAGE')` is **false**. Eight migrations call
`cron.schedule()`; they are already applied so nothing re-runs them, but the next migration that
schedules a job would fail after the cutover. Grant it as part of the change rather than discovering
it later.

## Phases

### Phase 1 — Bootstrap (once per database, as a superuser)
```sql
CREATE EXTENSION IF NOT EXISTS vector;
CREATE EXTENSION IF NOT EXISTS citext;
CREATE EXTENSION IF NOT EXISTS pgcrypto;
CREATE EXTENSION IF NOT EXISTS pg_cron;
GRANT USAGE ON SCHEMA cron TO sentinel;
```
This must live somewhere durable — `deploy/postgres/` initdb, or a documented runbook step — or the
next fresh deployment fails at migration 1 with an error that reads like a broken migration rather
than a missing bootstrap.

### Phase 2 — Drop the privileges
```sql
ALTER ROLE sentinel NOSUPERUSER NOBYPASSRLS;
```
One statement. Ownership, DDL and migrations are unaffected. **Rehearse against a restored copy of
production first** — this is the step that turns a working service into one that boots and then fails
its first query if anything above was missed.

### Phase 3 — Prove it
Invert `baseline_the_database_enforces_no_isolation_of_its_own` in
`crates/app-core/tests/tenant_isolation.rs`: the connecting role must NOT be a superuser and must NOT
carry `BYPASSRLS`. That test records the opposite as today's starting state, so the inversion is a
visible diff rather than a claim.

### Phase 4 — Then #160 becomes possible
With `BYPASSRLS` gone, `ENABLE` + `FORCE ROW LEVEL SECURITY` per table starts working. That is
[[tenant-isolation-rls]] Phase 2 onward, and stays a separate, larger card.

## Deferred — the per-service role split
The original plan proposed `sentinel_migrate` / `sentinel_app` / `sentinel_gateway` /
`sentinel_service`. Experiment 1 shows that is **not required** for either the RCE reach or for RLS.
It remains worthwhile defence in depth — the gateway serving public traffic does not need write
access to `billing` — but it is a refinement, not a prerequisite, and must not hold up the one-line
fix.

## Open decisions

1. ~~Who runs migrations.~~ **Answered by experiment 1**: ownership stays where it is, so every
   service keeps running `sqlx::migrate!` on boot and no deploy-ordering constraint is introduced.
2. **Where the bootstrap SQL lives** so a fresh database cannot miss it — `deploy/postgres/` initdb
   script, or a runbook step. Recommend initdb: a runbook step is a step someone skips.
3. **Whether the gateway needs the database at all**, or can go through control.
4. Whether to rotate the `sentinel` password during the cutover — it appears in plaintext in
   `/ibbf/docker-compose.sentinel.yml` (not in git; the repo uses `__DB_PASS__`). Related to #148.

## Related
[[tenant-isolation-rls]] (#160 is blocked on this) · [[security-hardening-remainder]] ·
[[vps-db-troubleshooting]]
