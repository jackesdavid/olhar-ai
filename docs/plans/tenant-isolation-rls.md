# Tenant isolation by construction — Implementation Plan

**Board card:** #160 Tenant isolation by construction — row-level security
**Goal:** Make cross-tenant data access **structurally impossible**, so a forgotten `WHERE tenant_id`
returns nothing instead of returning someone else's camera footage.

## Why now

Self-serve signup, payment and consent all shipped (#165, #166, #167). A stranger can now create a
workspace and point cameras at their home without anyone helping them. Until today the tenants were
us and a handful of pilots; the blast radius of a missing predicate was embarrassment. It is now
*video of one customer's house reaching another customer*.

Isolation is currently enforced by writing `tenant_id = $n` by hand in every query. The
[[architecture-weaknesses]] audit recorded the honest version: *"Twice today I reasoned from
`SELECT … FROM devices` with no filter and drew wrong conclusions about a customer's fleet. If it is
that easy to omit in a read, it is that easy to omit in a write."*

Nothing catches it. `check:rust` cannot — sqlx queries are runtime strings. `check:sql`
(`sql_prepare`) verifies every statement PREPAREs and that bind counts match, but it has **no opinion
about whether a tenant predicate is present**. A query that reads every tenant's rows is, to every
gate we own, a perfectly healthy query.

## The measurements that shape this card

Taken against `main` and production, 2026-08-05:

| Fact | Value | Consequence |
|---|---|---|
| Tables carrying `tenant_id` | **47** | the surface to protect |
| Total sqlx call sites | **330** | the surface to convert |
| Executed directly on the pool, no transaction | **178** | `SET LOCAL` cannot reach these |
| Executed inside a transaction | **62** | these can carry a tenant today |
| Tables owned by the connecting DB user (`sentinel`) | **62 of 62** | owners bypass RLS unless `FORCE`d |
| Connecting role is a **superuser** with `BYPASSRLS` | **yes, in production** | **RLS cannot work at all** |

### The trap that makes this card dangerous to do naively

Demonstrated against the local database rather than reasoned about, because the first version of
this section was **wrong**:

1. **Table owners bypass RLS.** Enabling RLS and adding a policy changed nothing — the owner still
   saw both tenants. The documented fix is `FORCE ROW LEVEL SECURITY`.
2. **`FORCE` did not help either.** Both tenants were still visible, and an unqualified
   `UPDATE … SET note = 'HACKED'` rewrote **both** rows. The reason: the connecting role is a
   **superuser** (`superuser=true bypassrls=true`, in production too). Superusers bypass RLS
   *unconditionally*; `FORCE` only makes policies apply to the table OWNER.
3. **As a plain non-owner role it works exactly as advertised.** `SELECT` with no `WHERE` returned
   one tenant's row; `UPDATE` with no `WHERE` rewrote only that tenant's row; switching the session
   variable switched the visible tenant; and a forgotten `SET LOCAL` failed closed with an error.

So the obvious first commit — `ALTER TABLE … ENABLE ROW LEVEL SECURITY` plus a policy — would deploy
clean, pass every test, and **change nothing at all**. That is the same shape as two failures already
found this month: the HNSW index never used because of a cast, and the `changes:` filter that skipped
every build while the pipeline stayed green. Assume this one will hide the same way, and design the
proof first.

## The two available strategies

### A. Row-level security (what the card names)
Real defence in depth: the database refuses, whatever the application forgets.

The cost is the tenant has to reach the database **per query**. The mechanism is a session variable
(`SET LOCAL app.tenant_id`) read by the policy via `current_setting`. `SET LOCAL` is
**transaction-scoped** — which is exactly why the 178 pool-direct call sites matter: there is no
transaction for the setting to live in, and a session-level `SET` on a *pooled* connection that is
later handed to another tenant's request is a cross-tenant leak far worse than the bug being fixed.

### B. A repository layer where a tenant-scoped query cannot be constructed without the tenant
The plan note's own fallback. Cheaper, catches omissions at **compile time** rather than runtime, and
needs no database role work. It does not protect against a hand-written query that bypasses the
layer, so it is a weaker guarantee — but a weaker guarantee that actually ships beats a stronger one
that stalls at 330 call sites.

**Recommendation: both, in this order — B's proof harness first, then A incrementally.** The
sections below sequence that.

## Phases

### Phase 1 — A failing test that proves the gap (do this first)
Mirroring `face_knn_bench.rs`: measure the property, do not assert the fix.

- Seed **two** tenants with overlapping data in the highest-risk tables (`faces`, `clips`, `events`,
  `devices`, `workflows`).
- Drive the real repo functions as tenant A and assert **nothing** belonging to B is ever returned.
- Include a deliberate negative control: a query with the tenant predicate removed must make the
  test **fail**, proving the harness can detect the thing it exists to detect.

Without the negative control this test will pass forever regardless of whether isolation works.

**Phase 1 is BUILT — and until 2026-08-18 it never ran.** `crates/app-core/tests/tenant_isolation.rs`
has seven tests including the negative control, and they all pass. Nothing executed them: `check:sql`
runs only `sql_prepare`. A test nobody runs is worth exactly what no test is worth, which is the
failure mode this very plan warns about two sections up. `check:tenant-isolation` in `.gitlab-ci.yml`
is what makes it a gate.

Two things learned running it, recorded so the next person does not lose an hour:

- **It must run `--test-threads=1`.** Every test migrates the same database on connect; seven racing
  produces `constraint "devices_tenant_fkey" already exists`, which reads like a schema bug and is
  not one.
- **It cannot run against a scratch database.** pg_cron refuses to create its extension anywhere but
  the database named in `cron.database_name`, so the database must be called `sentinel` — a fresh
  container, not a second database inside an existing one.

Measured on a clean run: `superuser=true bypassrls=true; tables with RLS enabled: 0`. The test
connects as the OWNER, so it proves the application's predicates work — it says nothing about
whether RLS would hold, which is Phase 2's job.

### Phase 2 — Make RLS possible at all

> **UNBLOCKED as of 2026-08-12 — step 1 is already done.** This used to read "blocked on #173":
> the connecting role was `superuser=true bypassrls=true`, and a superuser bypasses RLS
> *unconditionally* (`FORCE ROW LEVEL SECURITY` only makes policies apply to the table OWNER, which
> does nothing for a superuser). That is fixed. Measured on production:
>
> | role | `rolsuper` | `rolbypassrls` |
> |---|---|---|
> | `sentinel` (migrations, owner) | t | t |
> | `sentinel_app` (what control connects as) | **f** | **f** |
>
> So policies would now actually apply. **Nothing else here has been built**: `0 of 64` public
> tables have `relrowsecurity` set. Steps 2 and 3 below are untouched work, and the cutover risk
> in the closing paragraph still stands.
1. ~~A non-owner role for the application (`sentinel_app`): `GRANT SELECT/INSERT/UPDATE/DELETE` on
   the 47 tenant tables, owning nothing. Migrations keep running as `sentinel`.~~ **Done** (#173).
2. `ALTER TABLE … ENABLE ROW LEVEL SECURITY` **and** `FORCE ROW LEVEL SECURITY` on those tables.
3. A `BYPASSRLS` service role for the paths that are legitimately cross-tenant: the master console,
   the pg_cron sweeps (events retention, TTLs), and the `work_jobs` queue consumers.

**The mechanism is now PROVEN, in tests, before any migration (2026-08-18).** `tenant_isolation.rs`
gained two:

- `rls_does_nothing_for_a_superuser_or_owner_which_is_the_whole_trap` — enables RLS, adds a policy,
  and asserts the owner **still sees both tenants**. That is the state a naive "enable RLS" commit
  ships, and it is indistinguishable from working unless someone counts rows. Output:
  `trap confirmed: RLS enabled + policy present, owner still sees 2 rows`.
- `rls_isolates_a_non_owner_role_and_fails_closed_without_the_session_variable` — with `FORCE` on and
  a non-owner role, it asserts all four properties: a query with no `app.tenant_id` **errors**
  (fails closed, not "returns nothing"), an unqualified `SELECT` sees one tenant, an unqualified
  `UPDATE` rewrites one row, and the other tenant's row is byte-identical afterwards.

Both run on a scratch `rls_probe` table and role, created and dropped by the test, so nothing
application-owned is touched. The policy deliberately uses `current_setting('app.tenant_id')`
**without** the `missing_ok` argument: a forgotten `SET LOCAL` then raises, instead of silently
returning an empty list that a caller cannot distinguish from "no data".

So the migration can now be written against a mechanism known to work here, rather than hoping.
What remains is the risky half, unchanged:

Changing the connecting role is a production cutover with a real rollback story. It must be proven
against a restored copy of production before it touches the live database.

### Phase 3 — Carry the tenant to the database
Introduce a `TenantConn` (acquire → `BEGIN` → `SET LOCAL app.tenant_id` → run → `COMMIT`) and convert
call sites onto it.

**Not all 330 at once.** Convert per table, highest risk first — `faces` (biometric), then clips and
recordings (video), then `events`, then the rest. Each table can be `FORCE`d the moment its readers
are converted, so isolation lands incrementally and every step is independently verifiable.

### Phase 4 — Make omission impossible to reintroduce — **BUILT 2026-08-18**

`every_tenant_scoped_statement_names_the_tenant` in `sql_prepare.rs`. It asks the live schema which
tables carry `tenant_id` (**50**, not a hand-kept list that would drift), works out which tables each
of the 335 extracted statements touches, and requires the statement to mention `tenant_id`.

**Measured before designing the gate: 60 statements across 14 files fail that.** So the plan's
"allow-list with a reason" was the wrong shape — 60 justifications written in one sitting is
rubber-stamping, and a file of unread reasons is worse than an honest number. It is a **ratchet**
instead (`TENANT_PREDICATE_BASELINE`, per file):

- a file may not EXCEED its number → a new unscoped statement fails CI;
- a file may not fall BELOW it without the number being lowered → an unrecorded improvement can be
  silently undone later.

Per file, not per line: line numbers churn and a baseline nobody can keep current gets deleted.

**Proven by planting one.** A `SELECT count(*) FROM devices` added to `invites.rs` moved that file
from 2 to 3 and failed the gate, naming the file, the line and the table. Without that step this
would be another test that passes forever.

**What the 60 actually are.** Mostly `WHERE id = $1` — a row addressed by its own unguessable UUID,
already resolved by a tenant-scoped query upstream. Safe by unguessability, not by construction;
closing exactly that is what Phase 2 is for. The rest are legitimately cross-tenant: queue
consumers, the master console's `COUNT(*) FROM telegram_config`, invite acceptance by token.

**What it does not check.** That `tenant_id` is used as a PREDICATE — naming it in a SELECT list
passes. Tightening that needs a real SQL parser; a crude gate running today beats a correct one
that never lands.

## Open decisions

1. **Session variable vs a per-tenant Postgres role.** The variable is far simpler; a role per tenant
   is stronger and does not scale to self-serve signup. Recommend the variable.
2. **Whether `sentinel-flow` and `sentinel-notification` get the app role or the service role.** Flow
   executes tenant graphs (app role) but also runs cross-tenant schedulers (service). It may need
   both, which means two pools.
3. **Order of Phase 3 conversions** — proposed above by sensitivity, but billing tables may deserve
   to jump the queue.

## Out of scope
- Per-tenant database or schema separation. A different product decision with different economics.
- The edge appliance: it holds one tenant's data by construction and talks to control, not Postgres.

## Related
[[architecture-weaknesses]] · [[multitenant-saas-migration]] · [[validate-migrations-live-db]] ·
[[sql-prepare-ci]]
