# `blob unknown to registry` — the shared build cache

**Board card:** #183 CI: image push races the build cache and fails ~once a day
**Goal:** Stop losing a deploy cycle a day to a push failure that has nothing to do with the code.

## The symptom

`build:rust` (and `build:postgres`) fail at the very end, after compiling successfully:

```
ERROR: failed to push registry.gitlab.com/.../sentinel-flow:latest:
       unknown: blob unknown to registry - sha256:af47dea…
```

Four occurrences in three days (2026-08-12 ×2 on `sentinel-flow`, 2026-08-14 on
`sentinel-postgres` and `sentinel-flow`). Retrying always works, which is what made it look like
GitLab being flaky rather than something we do.

## The cause

`.gitlab-ci.yml`, `build:rust`: four services were built in one job, each exporting a `mode=max`
registry cache to **the same ref**:

```sh
CACHE="$IMAGE_PREFIX/buildcache:rust"
for SVC in sentinel-control sentinel-flow sentinel-gateway sentinel-notification; do
  docker buildx build --cache-to "type=registry,ref=$CACHE,mode=max" --push …
```

That costs twice over:

1. **The cache barely worked.** Each `mode=max` export REPLACES the manifest at `$CACHE`, so after
   a pipeline the stored cache is only ever `sentinel-notification`'s. The other three were
   exporting a full cache every run and reading someone else's on the next one.
2. **It races the registry.** Four cache exports and four image pushes interleave on the same
   repository. A push references a blob the concurrent cache export is still writing (or has just
   swept), and the registry answers `blob unknown`.

## The fix

One cache ref per service — `buildcache:rust-$SVC`. The exports stop clobbering each other, the
cache becomes real for all four, and nothing writes the same manifest twice in a job.

## Round 1 did not fix it (2026-08-14)

Per-service refs shipped, and the very next pipeline failed the same way — on
`sentinel-notification`, the LAST service, with four separate cache refs in play. So the race is
not between services competing for one ref: it is between a single build's own `--push` and its
own `--cache-to`, both writing blobs to the same repository at once.

Round 2 is `image-manifest=true,oci-mediatypes=true` on `--cache-to`, which stores the cache as a
standard OCI image manifest rather than the buildkit-specific shape. That is the documented fix for
this error against registries that verify blob references, GitLab's included.

The per-service refs stay regardless — the cache-clobbering they fixed was real and separate.

## If it recurs

The next lever is `--cache-to type=registry,…,image-manifest=true,oci-mediatypes=true`, which
stores the cache as a proper image manifest and is friendlier to registries that GC aggressively.
After that, splitting the four services into parallel jobs removes the interleaving entirely — at
the cost of four checkouts.

## Out of scope

- Retry-on-failure as the answer. It hides the cache thrash, which is a real cost of its own:
  three of four services were recompiling from a stranger's cache every pipeline.
