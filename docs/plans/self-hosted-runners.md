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

## Success test

A `v*` tag produces **all three platforms in one pipeline** — Linux x86_64, Linux aarch64, and a
signed, notarized macOS `.pkg` — with `manifest.json` showing one version across the board, and zero
shared minutes consumed. At that point #163 is closed and "nothing is built locally" is finally true
without an exception.
