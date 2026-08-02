# Deploy must assert what it deployed — Implementation Plan

**Weakness:** [[architecture-weaknesses]] §3 **Board card:** #158 Deploy must assert what it deployed
**Goal:** make "deploy succeeded" mean the new images are running, not that ssh returned 0.

## Why

`deploy:vps` ran `pull` + `up -d` and reported success whether or not the images moved. For the
whole GitLab migration it "succeeded" while production stayed frozen on the previous images,
because the compose file still pointed at the old registry. **Nine merges were built, stored, and
not running — and the pipeline was green throughout.**

This is the same family as #157: a check whose success condition can be met without the thing being
true. The assertion has to be inverted — state what should be running, then go look.

## Design

The build stage already writes one marker file per rebuilt service (`built/<service>`), which is how
deploy knows what to restart. That file now carries the evidence too:

```
built/sentinel-control  →  registry.gitlab.com/…/sentinel-control:latest sha256:<digest>
```

Three build paths produce markers, so each needs the digest the registry actually assigned:

| Path | Jobs | Source of the digest |
|---|---|---|
| `build:rust` | control, flow, gateway, notification | `docker buildx --metadata-file` → `containerimage.digest` |
| `.build_image` | postgres, ai-cpu | same |
| `.build_service` | app, landing | the `digest: sha256:…` line of `docker push` output |

After `up -d`, the remote script resolves each service's container, reads the image's repo digest,
and compares. Any mismatch — or a service with no running container — fails the job and prints both
digests.

**State goes through files, not variables.** `while read` runs in a subshell, so a failure counter
incremented in the loop is discarded when the loop ends: the assertion would pass by losing its own
findings. Failures append to `/tmp/deploy-failed.txt` and the job fails if that file is non-empty.

## Verified before shipping (2026-08-01)

The premise — that buildx's reported digest is the same string the VPS ends up with in
`RepoDigests` — was tested end to end against a throwaway local registry rather than assumed:

- **buildx path**: pushed digest `sha256:e01e59…` == `RepoDigests` after pull. MATCH.
- **plain push path**: `sed` extracted `sha256:5266bf…` from push output == `RepoDigests`. MATCH.
- **the outage shape**: pushed a new image to the tag and left the container running the old one.
  The assertion FAILED, printing expected vs running. This is the case that was silent before.
- **loop behaviour**: a matching digest passes, a wrong digest fails, and a service with no
  container fails with its own message.

## Out of scope (v1)

- Services the pipeline did not rebuild. The marker set is the claim; unrebuilt services are not
  part of it and are left alone.
- Health checks. This asserts identity, not that the container works — #159 (media smoke test) is
  the one that asserts behaviour.
- Rollback on mismatch. Failing loudly is the fix; deciding what to do about it is an operator call
  while the failure mode is still unfamiliar.
