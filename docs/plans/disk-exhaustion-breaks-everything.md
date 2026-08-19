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
