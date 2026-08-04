# Self-hosted runners — build everything on our own hardware

**Board card:** #168 CI compute budget **Also closes:** #163 (macOS CI runner — the last local build)
**Goal:** CI that cannot run out of minutes, and a macOS package built by CI rather than by hand.

## Why

GitLab shared minutes are exhausted (`ci_quota_exceeded` on every job of !23; namespace
`ultimate_trial`, 400-minute limit). `main` is protected and requires `check:rust`, so **nothing can
merge or deploy**. Buying more minutes is not an option.

**Self-hosted runners do not consume shared minutes on any plan, including Free.** That turns the
quota from a recurring bill into a non-issue, and it is the same move that finally closes #163: the
Mac that signs the `.pkg` becomes a CI runner, so the last hand-built artifact becomes a pipeline
job without changing where it physically runs.

## Architecture

| Runner | Executor | Tags | Runs |
|---|---|---|---|
| `jack-mac-docker` | docker (privileged) | `linux`, `docker` — **accepts untagged** | Every existing job, unchanged: `check:rust`, `check:sql`, `check:media`, `build:*`, `deploy:vps`, the Linux edge release |
| `jack-mac-shell` | shell | `macos`, `signing` — **untagged: no** | Only the macOS `.pkg` build + notarization |
| *(tonight)* Windows box | shell | `windows` | The Windows edge (#80/#83) |

Accepting untagged jobs on the docker runner is the load-bearing choice: **no `.gitlab-ci.yml`
change is needed** for the existing pipeline to move onto our hardware. Only the new macOS job needs
a `tags:` line. The Mac runs Linux containers through OrbStack, so `image: rust:1-bookworm` and the
`docker:27-dind` services behave exactly as they do on GitLab's runners.

## Already done (2026-08-02)

- Both runners **registered and online** as project runners of `olharai-grupo/OlharAI`.
- Tags and `run_untagged` verified via the API — the list endpoint does not echo tags, the detail
  endpoint does:
  - `54787510` docker — `run_untagged: True`, tags `[linux, docker]`
  - `54787511` shell — `run_untagged: False`, tags `[macos, signing]`
- `concurrent = 4`, and persistent `gitlab-cargo-registry` / `gitlab-cargo-target` volumes so Rust
  jobs are not a cold build every time.
- Started via `brew services` as a **user LaunchAgent**, not a root daemon — deliberate: `codesign`
  and `notarytool --keychain-profile sentinel-notary` need the **login keychain**, which a root
  daemon cannot reach. It restarts on login.

## Tonight — remaining steps

1. **Verify the docker runner end to end.** Re-run !23's pipeline and confirm `check:rust`,
   `check:sql` and `check:media` pass on `jack-mac-docker`. `check:media` is the interesting one:
   it needs the `mediamtx` service container alongside the job.
2. **Disable shared runners for the project** once (1) is green, so jobs stop being offered to the
   exhausted quota and failing:
   `glab api --method PUT projects/84911918 -f shared_runners_enabled=false`
   Do this **after** (1), not before — otherwise a broken local runner leaves no fallback.
3. **Add `release:edge-macos`** to `.gitlab-ci.yml`, `tags: [macos]`, `rules: $CI_COMMIT_TAG =~ /^v/`.
   It runs the script that already works by hand:
   ```yaml
   variables:
     MACOS_APP_CERT: "Developer ID Application: Jackes Lemos (3A4H2Y6ZVM)"
     MACOS_PKG_CERT: "Developer ID Installer: Jackes Lemos (3A4H2Y6ZVM)"
     NOTARY_PROFILE: sentinel-notary
   script:
     - ./deploy/macos-pkg/build.sh --sign
   ```
   **No certificate material goes into GitLab** — the keychain is already on the machine. That is
   strictly better than the hosted-runner design, which would have required exporting `.p12` files
   into CI variables.
4. **Extend `release:publish`** to take the `.pkg` as an optional artifact and `merge-manifest.sh`
   it as `macos`, so all three platforms publish from one place and the manifest keeps recording
   only what actually built.
5. **Windows runner** — register with `--executor shell --shell powershell`, tag `windows`, then a
   `release:edge-windows` job for #80/#83.

## Risks worth naming

- **The Mac must be awake.** A laptop asleep is a pipeline that queues forever rather than failing
  loudly. Worth `caffeinate` or Energy Saver settings if this becomes the only CI.
- **One machine is now the whole build farm.** Losing it loses CI, releases, and the signing
  identity at once. The certs should be backed up independently of this machine.
- **`privileged = true`** on the docker runner is required by the `docker:27-dind` services the
  image builds use. It is a real privilege grant on a machine that also holds the signing keychain.
- **Job logs are visible to project members**, and the shell executor runs as your user in your home
  directory. Keep the shell runner's jobs limited to the tagged macOS build.


## Three config traps — every one presents identically (2026-08-03)

A job sits at `running` forever, its log frozen at *"Preparing the docker executor"*, and **no
container is ever created**. That single signature had three distinct causes. Recognise it and check
all three, because the next runner (Windows, #169) will hit them too.

| Trap | Symptom | Fix |
|---|---|---|
| OrbStack has **no** `/var/run/docker.sock` | Runner defaults to the missing path and hangs silently | `host = "unix:///Users/<you>/.orbstack/run/docker.sock"` |
| `pull_policy` defaults to **`always`** | Re-pulls every image every job and hangs on this daemon, while a manual `docker pull` of the same image takes 11s | `pull_policy = ["if-not-present"]` |
| **Private** registry service images | `check:sql`'s `sentinel-postgres` is in our private registry; the runner has no credentials and sat 59 minutes on the pull | `docker login registry.gitlab.com` on the host, and pre-pull the image |

The final config additions under `[runners.docker]`:

```toml
host = "unix:///Users/jackesdavid/.orbstack/run/docker.sock"
pull_policy = ["if-not-present"]
```

**Diagnosis note.** Two wrong theories cost real time here: "42 minutes is plausible for three cold
Rust builds" (it was hung, not slow) and "the LaunchAgent environment differs from a shell" (it does
not — the config was wrong for both). The thing that actually settled it every time was the *job log
tail* and `docker ps`, not reasoning about what ought to be true.

## Verified working (2026-08-03)

All three jobs green on `jack-mac-docker`, and each proved it did real work rather than skipping:

- `check:rust` — 289 s
- `check:sql` — `sql_prepare: 299 statements extracted, 297 prepared, 297 bind-count checked`
- `check:media` — `media_smoke: RAN frames=7368 B, clip=120368 B`

Then re-verified under the `brew services` LaunchAgent (not a foreground process), so it survives a
reboot. **Shared runners are now disabled for the project** — the exhausted quota can no longer be
offered a job.

**OrbStack must be running.** It was down at the start of this session and the runner had no daemon
at all. Set it to launch at login, or CI silently has zero capacity.


## #170 — the pull hang is worked around, not fixed

Four jobs hung on four different images (`rust:1-bookworm`, the runner helper, the private
`sentinel-postgres`, and `registry.gitlab.com/security-products/secrets:7`). Each was cured by
pulling that image **by hand** on the host first. `secret-detection` went from hanging indefinitely
to **23 seconds** purely from having the image cached.

So the rule today is: **any image not already local hangs the first time the runner pulls it**,
while `docker pull` of the same reference from a shell takes 10–25 s. Root cause unknown — most
likely how the runner's Docker API client consumes the pull-progress stream against OrbStack.

**Consequence to remember:** adding any new `image:` or `services:` entry to `.gitlab-ci.yml` will
hang its first job, with a frozen log and no container as the only symptom. Pre-pull it on the host
until #170 is understood.

## The macOS release job (#163)

`release:edge-macos` runs on the `macos` tag (the shell executor), calls the same
`deploy/macos-pkg/build.sh --sign` that has always worked by hand, asserts `spctl` accepts the
result, and hands the `.pkg` to `release:publish` as an artifact. `allow_failure: true` — a sleeping
Mac must not fail a release Linux can still complete, and `release:publish` prints an explicit NOTE
when no package arrived rather than publishing a Linux-only release in silence.


## Success test — PASSED on v0.5.73 (2026-08-03)

One tag, five jobs, all on our own hardware, zero shared minutes:

```
verify:edge-lock      success   24 s
release:edge-x86_64   success             jack-mac-docker
release:edge-aarch64  success             jack-mac-docker  (cross-compiled)
release:edge-macos    success             jack-mac-shell   ← first ever run
release:publish       success
```

The macOS job's own log, from the runner:

```
The staple and validate action worked!
Sentinel-Edge-0.5.73-gst-x86_64.pkg: accepted
source=Notarized Developer ID
```

And the manifest agrees across the board — **all three platforms at 0.5.73**, all downloadable
(HTTP 200):

| platform | file | version |
|---|---|---|
| macos | `Sentinel-Edge-0.5.73-gst-x86_64.pkg` | 0.5.73 |
| linux-x86_64 | `sentinel-edge-linux-x86_64-0.5.73` | 0.5.73 |
| linux-aarch64 | `sentinel-edge-linux-aarch64-0.5.73` | 0.5.73 |

Both first-run risks cleared: the shell executor found `cargo` (the `PATH` export works from a
LaunchAgent) and `codesign`/`notarytool` reached the **login keychain** — which is exactly why the
runner is a user agent and not a root daemon.

**#163 is closed and "nothing is built locally" is now true without an exception.**

## Original success test (for reference)

A `v*` tag produces **all three platforms in one pipeline** — Linux x86_64, Linux aarch64, and a
signed, notarized macOS `.pkg` — with `manifest.json` showing one version across the board, and zero
shared minutes consumed. At that point #163 is closed and "nothing is built locally" is finally true
without an exception.

## Success test — PASSED (v0.5.73, 2026-08-03)

One tag, one pipeline, three platforms, zero shared minutes:

```
macos          0.5.73  Sentinel-Edge-0.5.73-gst-x86_64.pkg
linux-x86_64   0.5.73  sentinel-edge-linux-x86_64-0.5.73
linux-aarch64  0.5.73  sentinel-edge-linux-aarch64-0.5.73
```

All three answer 200 from `/edge/`. `release:edge-macos` succeeded on its FIRST run — both
first-run risks (the shell executor's `PATH` finding cargo, and an unlocked login keychain) turned
out fine — and its `spctl` assertion passed: `accepted / source=Notarized Developer ID`.

**"Nothing is built locally" is now true without an exception.** The `.pkg` hand-built since card
#82 is a pipeline job: same physical Mac, but reproducible, logged, and triggered by a tag rather
than by someone remembering the steps.

Prerequisite that bit twice: **OrbStack was down at the start of BOTH sessions**, and the runner
stays "online" to GitLab regardless — so CI had silently zero capacity while looking healthy. Set it
to launch at login. `caffeinate -dimsu` during a release is worth it too; a sleeping Mac queues
forever rather than failing.
