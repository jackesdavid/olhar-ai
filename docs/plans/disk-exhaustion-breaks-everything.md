# The disk fills, and it takes CI and the agent down with it

**Board card:** #197 Disk exhaustion breaks builds and the cleanup task can't reclaim the cause
**Goal:** Free space where it actually accumulates, without a manual step that risks the dev data,
and notice the problem before a build dies of it.

## What happened (measured 2026-08-18)

Twice in one day, and each time it broke something different:

1. **CI failed.** `!116`'s `check:rust` died with `No space left on device (os error 28)` writing
   rlibs under `sentinel-edge/target`. Nothing wrong with the code — the runner shares this
   machine's disk.
2. **Every local command failed.** Not just builds: the agent's shell tool writes its output file
   to the same volume *before* running the command, so `df -h` itself returned `ENOSPC`. There was
   no way to diagnose or fix it from inside the session — it needed a human at a terminal.

Free space went 12 GiB → 8.1 → 5.3 over an afternoon of ordinary work.

## Where the space actually is

| | Size | Reclaimable |
|---|---|---|
| Docker **local volumes** | **24.79 GB** | **17.93 GB (72%)** |
| Docker images | 2.99 GB | 0 B (all active) |
| Local `target/` | 849 MB | all |
| Containers, build cache | ~0.5 MB | ~0 |

The volumes are the problem, by an order of magnitude. The largest single one is
`gitlab-cargo-target` at **5.05 GB**, plus `bishop-target` (1.59 GB) and three cargo registries.

## Why `task clean-space` cannot fix this

The task runs `cargo clean` on both trees, drops the installer build dirs, then
`docker system prune -af`.

**`docker system prune -af` does not prune volumes.** `--volumes` is a separate flag the task does
not pass, so `system prune` removes images, stopped containers, networks and build cache — none of
which is where the space went. The task reclaims the 849 MB it can see and leaves the 17.93 GB it
cannot.

Its `-a` also carries the **#170 trap**: removing every image not held by a *running* container
takes out images the next pipeline stage needs, and the job then hangs at "Preparing the docker
executor" re-pulling them. That makes the task unsafe to run at the exact moment it is most needed
— when a queue of MRs is building.

## The trap for whoever reaches for `docker volume prune`

It is the obvious next move and it is dangerous here. `volume prune` deletes every volume not
attached to a **running** container, and with the compose stack stopped that list includes:

- `bishop_sentinel-pgdata` — the local dev Postgres
- `bishop_sentinel-minio` — the local object store (clips, snapshots)

They are dangling because nothing is running, not because they are garbage. A blanket prune during
a quiet moment silently destroys the local dev database.

## Phases

### Phase 1 — make `clean-space` reclaim the right thing, safely
Replace the blanket prune with an explicit, named list of build-cache volumes
(`gitlab-cargo-target`, `gitlab-cargo-registry`, `bishop-target`, `bishop-cargo-registry`,
`cargo-registry`) and drop `-a` from the image prune, or gate the image prune on no runner
container being present. Named deletion is the point: it cannot take data volumes with it, and it
cannot be widened by accident.

A volume held by a running job simply fails to delete — which is the correct behaviour and needs no
special case.

### Phase 2 — notice before it breaks something
A `task disk` that prints free space and the top consumers, and a CI job that fails loudly with
"the runner host is low on disk" rather than a compiler error about an rlib. `check:rust` reporting
a link failure for a disk problem is what made the first hour of this confusing.

### Phase 3 — stop the accumulation
`gitlab-cargo-target` grows without bound because nothing ever trims it. Either cap it, or accept
the rebuild cost and delete it on a schedule. Decide which; today it is neither.

## Verification criteria

1. `task clean-space` reclaims the cargo/target volumes and free space rises by GB, not MB.
2. Running it while a CI job is in flight does not break that job.
3. `bishop_sentinel-pgdata` and `bishop_sentinel-minio` survive it, with the compose stack stopped.
4. A build that would fail for disk says so.

## Out of scope
- Buying a bigger disk. It would postpone this, not answer it.

---

## Second measurement (2026-08-24) — the Docker numbers were not the cause

It happened again, mid-session, and produced the exact failure mode described above: the shell
tool could not write its own output file, so `df` itself returned `ENOSPC`.

The 2026-08-18 measurement looked at Docker and found 24.79 GB of volumes. That is real, but it
is not what fills this disk. `task clean-space` ran and reclaimed **791 MB**. Then a wider
measurement found where the space actually is:

| | Size | Reclaimable | Note |
|---|---|---|---|
| **Xcode `iOS DeviceSupport`** | **16 G** | all | a full symbol set per iOS build per device ever plugged in |
| Xcode `DerivedData` | 6.1 G | all | |
| `~/go/pkg/mod` | 6.8 G | all | |
| `CoreSimulator/Devices` | 5.2 G | partly | `simctl delete unavailable` only |
| `~/.npm` | 1.7 G | all | |
| `~/.cargo/registry` | 1.2 G | all | keep `index`, drop `src` + `cache` |
| Xcode `Archives` | 1.9 G | **no** | shipped builds; needed to symbolicate or resubmit |
| `~/builds` (runner) | 1.9 G | **no** | clearing mid-job kills the job |

`iOS DeviceSupport` alone is twenty times what the old task reclaimed, and it appeared the moment
this repo started building on a phone (#144, #202). It regenerates on the next device connect —
a few minutes, once.

**Fixed:** `scripts/clean-space.sh` now covers all of the above. Measured on this machine:
**29 G reclaimed, 5.3 GiB → 38 GiB free.**

Two things the script does beyond deleting:

- It names what it **left alone and why** — archives, simulator devices, runner workspaces,
  Docker volumes. The last one is the 2026-08-18 finding, and it stays manual on purpose: the
  local Postgres from `task db-up` lives there, and losing a dev database to a cleanup command
  is a bad trade.
- It prints the biggest directories still on disk. The complaint in the card title was that the
  task could not reclaim the cause; the durable fix is that it now *reports* the cause, so the
  next time the list is different it says so instead of claiming success.

## The failure mode is still worth stating plainly

**This machine is the CI runner.** When the disk fills, jobs do not fail with a compile error —
the OrbStack daemon dies and every job reports `system failure: cannot connect to the Docker
daemon`. Twice today that took out `check:media` and `check:tenant-isolation` on a merge request
whose code was fine, and it reads exactly like a flake.

## Still open

- **Nobody is watching.** Everything above is a cure. There is no alert at, say, 15 GiB free, so
  the first symptom is still a dead pipeline or a shell that cannot run `df`.
- Docker volumes (24.79 GB, 72% reclaimable per the first measurement) remain manual.


---

## The cleanup was making builds slower (2026-08-25)

`clean-space` ran `docker system prune -af`, which takes the **build cache** along with the
unused images. This machine is the CI runner, so that cache is what a `cargo chef` layer is
restored from — and without it every Rust build starts from nothing.

Measured across the runs of one afternoon: `build:rust` went **24 → 25 → 43 minutes**, and
`docker system df` reported `Build Cache 0B` after each cleanup. The cleanup was buying disk
with about twenty minutes of every subsequent build.

**Fixed:** `docker image prune -af` for the images, and `docker builder prune -f --filter
until=168h` for the cache — trimmed rather than emptied. A week is long enough that an ordinary
week of commits keeps hitting it, and short enough that a branch nobody has touched since is not
being stored for free.

What that costs in reclaimed space is small: at the time of the change, `docker system df`
reported 4.999 GB of images with 1.473 GB reclaimable. The images were never where the space
was — **local volumes are 28.94 GB with 12 GB reclaimable**, which is the same finding as the
first measurement and is still deliberately manual, because the local Postgres lives there.
## The daemon also dies with plenty of disk (2026-08-25)

Three times in one day every job in a pipeline failed at once with:

    ERROR: Job failed (system failure): getting docker info: Cannot connect to the Docker
    daemon at unix:///Users/jackesdavid/.orbstack/run/docker.sock

The first two were the disk. **The third was not** — 24 GiB free at the time. OrbStack had simply
stopped, and nothing restarts it: the runner keeps accepting jobs and failing every one of them
in about ten seconds.

That failure reads exactly like a broken commit if you only look at the pipeline page: seven red
checks, all at once, on a branch whose code is fine. The tell is the **duration** — ten seconds
for a job that normally takes minutes means it never started, and a job that never started is
never the code's fault.

**Recovery is `open -a OrbStack`, wait for `docker info`, then retry the pipeline.** No state is
lost; the jobs re-run from scratch.

**Still not fixed, and now more clearly worth fixing:** nobody is watching. A dead daemon and a
full disk produce the same symptom — a wall of red — and both are invisible until somebody looks.
A check that runs before a job and refuses to start it (rather than failing it) would turn both
into a queued pipeline instead of a false accusation against a commit.
