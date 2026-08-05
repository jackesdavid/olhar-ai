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

## Phases

### Phase 1 — Measure the actual privileges each service uses
Before removing anything, find out what would break. `pg_stat_statements` (or a temporary log of
statement kinds per role) over a representative period: does anything genuinely need DDL at runtime?
Migrations do — they run on boot via `sqlx::migrate!` from every service that connects.

That last point is the real design question, and it deserves a decision rather than an assumption:
**every service currently runs migrations at boot**, so every service currently needs DDL rights.

### Phase 2 — Split the roles
- `sentinel_migrate` — owns the schema, runs migrations. Used by a boot/init step, not by request
  handlers.
- `sentinel_app` — no superuser, no ownership, no `BYPASSRLS`; `SELECT/INSERT/UPDATE/DELETE` on the
  tables it needs. What control/flow/notification use for request work.
- `sentinel_gateway` — narrower still if the gateway's real query set is small; measure in Phase 1.
- `sentinel_service` — `BYPASSRLS` for the legitimately cross-tenant paths (master console, pg_cron
  sweeps, `work_jobs` consumers), and nothing else.

### Phase 3 — Cut over
Roles are a production change with a rollback story: wrong grants mean a service that boots and then
fails its first query. Rehearse against a restored copy of production, deploy one service first
(notification is the smallest blast radius), then the rest.

### Phase 4 — Prove it
Extend `tenant_isolation.rs`'s `baseline_the_database_enforces_no_isolation_of_its_own`, which today
records `superuser=true bypassrls=true` as the starting point. When this card lands, that test
inverts: the connecting role must NOT be a superuser. That inversion is the proof.

## Open decisions

1. **Who runs migrations.** Keeping `sqlx::migrate!` in every service is simple and self-healing but
   forces DDL rights everywhere. A separate migration step is stricter and adds a deploy ordering
   constraint. This is the decision the card turns on.
2. **Whether the gateway needs the database at all**, or can go through control.
3. Whether to rotate the `sentinel` password during the cutover — it appears in plaintext in
   `/ibbf/docker-compose.sentinel.yml` (not in git; the repo uses `__DB_PASS__`). Related to #148.

## Related
[[tenant-isolation-rls]] (#160 is blocked on this) · [[security-hardening-remainder]] ·
[[vps-db-troubleshooting]]
