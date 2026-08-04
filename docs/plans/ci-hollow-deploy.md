# A merge to `main` can deploy nothing and report success — Implementation Plan

**Board card:** #171 Hollow deploy — a merge can skip every build job and still report success
**Goal:** Make it impossible for `deploy:vps` to report success when the thing it deployed is not the
thing that was just merged.

## What happened (2026-08-04, observed)

MR !27 merged #126 + #127 into `main` — a change touching `crates/**`, `sentinel-control/**` and
`sentinel-edge/**`, including two migrations. The resulting pipeline
([2728823791](https://gitlab.com/olharai-grupo/OlharAI/-/pipelines/2728823791)) contained:

```
check   check:rust    success
check   check:sql     success
deploy  deploy:vps    success   30s     ← deployed NOTHING
```

**No build stage ran at all.** Not `build:rust`, not `build:app` — the jobs were never created.

`deploy:vps` then behaved correctly and said so:

```
nothing was rebuilt — not touching the VPS
```

…and exited 0. The #158 digest assertion was never reached, because there was nothing to assert.
Production was left untouched on the previous images.

The only reason it was caught is that 30 seconds is visibly too short to build Rust. Manually
triggering a pipeline on the same commit
([2728839579](https://gitlab.com/olharai-grupo/OlharAI/-/pipelines/2728839579)) created all five
build jobs; `build:rust` took **2009 s**, and only then did the real code reach the VPS.

## Why

`build:rust` (and every other build job) is gated on a `changes:` filter:

```yaml
rules:
  - if: $CI_PIPELINE_SOURCE == "web"
  - if: $CI_COMMIT_BRANCH == $CI_DEFAULT_BRANCH
    changes:
      - sentinel-control/**/*
      - crates/**/*
      ...
```

On a push of a **merge commit**, GitLab evaluates `changes:` against `CI_COMMIT_BEFORE_SHA`. For a
merge whose source branch was already up to date with the target, the diff it compares is empty —
so nothing matches, and every build job is skipped. `deploy:vps` has no `needs:` on them
(`optional: true`), so it runs regardless.

The `web` rule is why a manual trigger works: a pipeline with no push event evaluates `changes:` as
true.

## Why it is dangerous, not just wasteful

The danger is **silent non-delivery**, not corruption. Nothing half-applied: no image was built, so
no migration ran and no code changed. Running code and schema stayed consistent at the previous
version. What was lost is the merge itself — #126 and #127 sat in `main`, believed shipped, running
nowhere, until someone noticed 30 seconds was too short to build Rust.

That is the whole failure: **a green pipeline that means "I did nothing".** `deploy:vps` reports
`nothing was rebuilt` and exits 0, which is right when a docs-only merge genuinely needs no build
and wrong when five build jobs were skipped by a filter that misfired. The message is identical in
both cases, and it scrolls past inside a green job.

Card #158's digest assertion cannot catch this: it compares running containers against
`built/`, and `built/` is empty, so the loop has nothing to iterate. It answers "is what we pushed
running?" — never "should we have pushed something?".

## Phases

### Phase 1 — Make the failure visible ✅ done
`deploy:vps` now FAILS when `built/` is empty on the default branch, instead of exiting 0 with
`nothing was rebuilt`. With Phase 2 in place every build job runs unconditionally on `main`, so an
empty `built/` can only mean a build job was skipped or failed to upload — which is a broken
pipeline, not a no-op. The permissive `exit 0` is kept for every other ref.

### Phase 2 — Stop skipping builds on `main`
Pick one:

1. **Drop `changes:` on the default branch.** Always build on `main`. Simplest and unambiguous; costs
   runner time on docs-only merges. Given the runners are ours (#168) and shared minutes are not
   being bought, this is close to free.
2. **Keep `changes:` but fix the comparison** — compare against the merge base rather than
   `CI_COMMIT_BEFORE_SHA`. More precise, but it depends on GitLab behaviour that is exactly what
   surprised us here, so it needs the Phase 3 test to be trustworthy.

**Recommend option 1** on `main` only, keeping `changes:` on merge-request pipelines where the
saving actually matters and a wrong skip is harmless.

**✅ Done** — the `changes:` block was removed from the default-branch rule of all five build jobs
(`build:rust`, `build:app`, `build:landing`, `build:postgres`, `build:ai-cpu`). Note these jobs
never ran on MRs to begin with (their rules are only `web` and `main`), so this changes merge
pipelines only. Cost: every merge to `main` now rebuilds all five images (~35 min wall-clock,
dominated by `build:rust`). On our own runners (#168) that is time, not money — and it is the price
of a merge that is actually deployed.

### Phase 3 — A test that would have caught it (NOT done)
Assert the *pipeline shape*, not just the job outcomes: on `main`, a commit touching `crates/**` must
produce a `build:rust` job. This can be a small job that queries its own pipeline via the API and
fails when a build job it expected is absent. Without this, Phase 2 is a fix nobody can prove stayed
fixed.

## Related history

This is the same class of failure as the note in [[sentinel-flow-deploy-gotchas]] — *"a docs-only
push after a code push silently un-deploys it — verify migrations in the live DB, not the CI UI"*.
That memory recorded the symptom; this card removes the cause. It is also the natural extension of
[[deploy-assert-digests]] (#158): that card asserted the deploy, this one asserts the build.

## Out of scope
- Reworking the release (tag) pipeline — it is triggered by `$CI_COMMIT_TAG` and does not use the
  `changes:` filter, so it is not affected.
