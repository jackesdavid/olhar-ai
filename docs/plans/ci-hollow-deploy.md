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
deploy  deploy:vps    success   30s     ← redeployed the EXISTING images
```

**No build stage ran at all.** Not `build:rust`, not `build:app` — the jobs were never created. The
deploy pulled and restarted whatever was already in the registry, asserted those digests, and went
green in 30 seconds.

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

Migrations are **embedded in the service binaries** (`sqlx::migrate!`, applied on `Db::connect` at
boot). A hollow deploy therefore does not merely ship stale code — it ships stale code *and* skips
the schema change that the merged code assumes exists. Code and database silently disagree, and the
pipeline says success.

Card #158 already made the deploy assert what it deployed, and that assertion **passed** here: every
running service genuinely matched the image in the registry. The assertion answers "is the registry
image running?" — not "was the registry image built from this commit?". That is the gap.

## Phases

### Phase 1 — Make the failure visible (cheap, do first)
Have `deploy:vps` compare the deployed image's source revision against `CI_COMMIT_SHA` and **fail**
when a service that *should* have been rebuilt was not. The images already carry a revision label
(or one can be added via `--label org.opencontainers.image.revision=$CI_COMMIT_SHA`); the deploy
already reads digests, so this is an extra assertion in the same script, not new machinery.

This alone converts a silent wrong outcome into a loud one.

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

### Phase 3 — A test that would have caught it
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
