# Architectural weaknesses — what must be fixed to grow healthily

Written 2026-08-01, after a day in which the product broke four separate times. Every item below is
grounded in something that actually failed, not in a style preference. Ordered by what would hurt
most, soonest.

The common thread: **the system has many ways to be wrong silently.** Most failures today were not
crashes — they were confident wrong answers, or things that quietly stopped happening. That is the
property to attack.

---

## 1. Images that nothing builds — and the database is one of them

`sentinel-postgres` and `sentinel-ai` appear in the VPS compose and in **no CI job**. They exist
only as whatever `deploy/push-local.sh` last produced by hand.

**How it already bit us.** `sentinel-notification` had the same gap. It ran fine for four days
because the container was *already running* and never re-read the schema. The registry cutover
restarted it and it could not boot — `migration 74 was previously applied but is missing in the
resolved migrations`. Notifications were down for two hours. The image had been stale for weeks;
the restart is merely when that became visible.

`sentinel-postgres` is the same trap, and it is the database. It bundles `pg_cron`, so stock
Postgres is not a substitute. GHCR is now frozen (GitHub archived), so the only copy is the mirror
pushed by hand.

**Fix.** A Dockerfile per image, wired into `build:rust`'s stage. Anything the compose file names
must be reproducible from source in CI. The rule worth adopting: *if it runs in production and CI
cannot rebuild it, it is not deployed — it is stranded.*

## 2. sqlx queries are strings the compiler never reads

320 `sqlx::query` call sites. None are compile-checked (the crate is built without the macro
feature, deliberately — it needs no live DB to build). No test connects to a database, so none are
covered.

**How it bit us.** Removing one bind from an `UPDATE` left every later placeholder pointing one
argument to the right. `gstreamer_ok` (boolean) received `platform` (text), Postgres refused the
COALESCE, and **every box token lookup failed**. `cargo check` passed. All 296 tests passed. The
edge could not authenticate until a fix shipped.

**Fix, cheapest first.**
- A test that `PREPARE`s every statement against a schema built from the migrations. It needs one
  throwaway Postgres and catches the entire class — placeholder drift, type mismatch, renamed
  column — at build time instead of in production.
- Until that exists: `PREPARE` a changed statement by hand before shipping. Thirty seconds.

## 3. Nothing verifies that a deploy changed anything

`deploy:vps` runs `pull` + `up -d` and reports success whether or not the images moved. For the
whole GitLab migration it "succeeded" while production stayed frozen on the previous images,
because the compose file still pointed at the old registry. Nine merges were built, stored, and not
running — and the pipeline was green throughout.

**Fix.** After `up -d`, assert the running image digests match what was just pushed, and fail the
job if they do not. A deploy that cannot tell you what it deployed is a notification, not a deploy.

## 4. Silent failure is the default, in several shapes

Four instances in one day:

| What | Why it was silent |
|---|---|
| Linux edge release failing for five versions | `release.yml` ran only on tags; nobody tagged. Unversioned filenames meant boxes re-downloaded a stale binary and reported success. |
| `install.sh` fix deploying nothing | The file is `include_str!`-compiled into control but was in no CI path filter. |
| Camera status frozen for hours | A stale cached frame kept the code in a branch that never re-probes. |
| `edge:full-disk` never firing | It watches the recording quota, not free space. A box at 99% is silent. |

**Fix.** These are one pattern: *a check whose success condition can be met without the thing being
true.* Each needs its assertion inverted — publish a manifest of what actually built (done), assert
the deployed digest (item 3), expire an inference (done for frames), alarm on the resource the user
would look at.

## 5. Nothing exercises the media path

The go2rtc/ffmpeg removal rewrote the capture supervisor, the clip muxer, and the snapshot path.
`check:rust` proves it compiles. **Nothing proves a camera still records.** The same is true of
every media change: it reaches a customer box before anything has watched it produce a frame.

**Fix.** A `gst-selftest`-style job that builds the real pipeline against a synthetic RTSP source in
CI and asserts frames out, a clip muxed, and a snapshot returned. The edge already ships
`gst-selftest` for the installer; extend it and run it on every merge that touches the media path.

## 6. Multi-tenancy is enforced by discipline, not by construction

Every tenant-scoped query carries `tenant_id` by hand. Twice today I reasoned from
`SELECT ... FROM devices` with no filter and drew wrong conclusions about a customer's fleet — once
recording it in a plan note as a product gap. If it is that easy to omit in a read, it is that easy
to omit in a write.

**Fix.** Postgres row-level security with the tenant set per connection, so omission fails closed
instead of returning someone else's rows. Failing that, a repository layer where a tenant-scoped
query cannot be constructed without the tenant.

## 7. One box, one region, one of everything

Single control container, single Postgres, single VPS. Fine at four boxes; not a growth path. Worth
naming now so it is a decision rather than a surprise: the first customer who needs an SLA needs
Postgres replication and a second control replica, and both are much cheaper to design for before
the schema has opinions about it.

---

## Not on this list, deliberately

**Build times** (57.9 → 20.0 min) and the **update path** (platform-aware, forward-only,
manifest-driven) were fixed today and need no further work.

**The private link** is gone in behaviour and, since #199, in code — the readers are removed and
the naming with them. `magicdns_host` survives (it still carries a directly addressable host in dev)
and the old state column is now inert with zero readers. Dropping that column is what remains, and
it is a migration, not an architectural weakness. The order still matters: a migration that drops a
column a runtime query still names is item 2 all over again.
