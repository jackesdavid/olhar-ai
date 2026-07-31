# Deploy & ops

## TL;DR

GitHub Actions is **billing-blocked** (the deploy/release jobs "were not started — recent
account payments have failed"). So we deploy **locally**:

```sh
task cloud-deploy                                   # all cloud services
task cloud-deploy -- sentinel-control sentinel-app  # a subset
# (equivalently: ./deploy/push-local.sh [services])
```

`deploy/push-local.sh` mirrors `.github/workflows/deploy.yml`: builds each image for
**linux/amd64** (the VPS arch) from the same Dockerfiles + build-args, pushes to
`ghcr.io/jackesdavid/<svc>:latest`, then SSHes the VPS to `pull` + `up -d` + prune (and
stamps `LATEST_EDGE_VERSION`). Prereqs: **Docker Desktop running**; `gh` logged in as
**jackesdavid** (has `write:packages`).

## The VPS

- Contabo, `root@85.239.248.86`, key `~/.ssh/id_contab_ed25519` (ssh alias `Contabo` exists
  but lacks an IdentityFile — pass `-i ~/.ssh/id_contab_ed25519 -o IdentitiesOnly=yes`).
- Compose at `/ibbf`: `docker-compose.yml` + `docker-compose.sentinel.yml`. Services:
  `sentinel-control / -gateway / -flow / -app / -landing` (+ `sentinel-postgres`,
  `sentinel-ai`). Migrations apply on control + flow boot.
- Two `gh` accounts are configured — run `gh auth switch -u jackesdavid` before API calls,
  and SSH/git use the SSH key (not the gh token), so pushes work even when `gh api` 404s.

## Versioning & release

- **Web app**: `sentinel-app/package.json` + the footer string in `routes/+layout.svelte`.
  The footer also shows the deployed commit (`VITE_COMMIT`, baked at build time).
- **Edge**: `sentinel-edge/Cargo.toml` `version`. This drives `LATEST_EDGE_VERSION` (the
  Boxes "update available" badge) and the `v*` release tag. Tags track the edge version.
  **Auto-bumped**: `task edge-publish` / `task edge-linux-publish` always bump the patch
  version first (`task edge-bump-version` — keeps `Cargo.lock`'s own entry in lockstep too),
  so a shipped edge binary's version always moves; you never need to (and shouldn't) hand-edit
  it before publishing.
- A "release" = bump web (by hand) + edge (automatic), commit, `task cloud-deploy`, and ship
  the edge binary (below). Current: **web 0.4.1, edge 0.5.0**.

## The edge installer + binary (served by control)

A tenant installs the appliance with one command. The installer + binaries are served by
control (the repo is private, so not GitHub Releases):
- `control/install.sh` — the canonical route, **`include_str!`'d into the control binary**
  (changing it normally needs a control rebuild).
- `control/edge/<asset>` — `ServeDir` over the VPS `/ibbf/edge-dist` (edge binaries + a copy
  of `install.sh`). **No rebuild needed to change these — just scp into `/ibbf/edge-dist`.**
- The Boxes page generates the install one-liner; it points at **`/edge/install.sh`** (so
  installer tweaks ship by scp, not a heavy control rebuild). `install.sh`'s `RELEASE_BASE`
  defaults to `$CONTROL_URL/edge`, so it fetches the binary from `/edge-dist` too.

**Shipping a new edge binary while Actions is down** (build locally + scp). The edge now links
**OpenCV** (the motion watcher) and is **out of the cargo workspace** — build it from its own
dir, with libopencv + libclang present:
```sh
cd sentinel-edge
# macOS (the current test box) — brew install opencv; pass libclang for the opencv bindgen:
LIBCLANG_PATH=/Library/Developer/CommandLineTools/usr/lib \
DYLD_FALLBACK_LIBRARY_PATH=/Library/Developer/CommandLineTools/usr/lib \
cargo build --release          # (or just `task edge`, which sets these)
scp -i ~/.ssh/id_contab_ed25519 -o IdentitiesOnly=yes \
  target/release/sentinel-edge root@85.239.248.86:/ibbf/edge-dist/sentinel-edge-macos-x86_64
```
**Linux appliance binaries (cross-build from macOS):** the old static-musl `cross` path no
longer works — OpenCV is a native C++ lib that needs glibc + dynamic linking. Instead we build
in a Fedora container (`deploy/edge-linux/Dockerfile`) and ship the binary. **Real appliances
are Bazzite** (immutable Fedora), so we build on Fedora to match the OpenCV soname:
```sh
task edge-linux-build      # → sentinel-edge/dist/sentinel-edge-linux-x86_64 (Fedora 42, OpenCV 4.11)
task edge-linux-publish     # build + scp to /ibbf/edge-dist + redeploy control (bumps LATEST_EDGE_VERSION)
# Bump FEDORA to match the appliance: task edge-linux-build FEDORA=41
```
The binary links OpenCV **dynamically**, so the run host's OpenCV major must match the build's
Fedora major. `install.sh` **auto-detects the OS**: macOS (launchd) / mutable Linux (`dnf`/`apt` installs
OpenCV + systemd unit) / **immutable Linux** (Bazzite / Silverblue — detected via
`rpm-ostree` and prints a **reboot instruction** (the layered libs activate on next boot; the
service is enabled but not started until then). It also echoes the edge version it's installing
avoids the reboot. `release.yml` still assumes `-musl` + `cross`; it needs a
Fedora-container job before CI can build Linux edges (CI is billing-blocked anyway).

**Shipping a new install.sh without a control rebuild**:
```sh
scp -i ~/.ssh/id_contab_ed25519 -o IdentitiesOnly=yes \
  sentinel-edge/install.sh root@85.239.248.86:/ibbf/edge-dist/install.sh
# served at https://control.thesentinels.online/edge/install.sh
```

## Gotchas learned the hard way

- **Disk**: building the heavy Rust images (control/flow/edge) locally eats GB fast (build
  cache + Docker.raw). If a build dies with `ENOSPC` (it also breaks the harness's command
  capture), `docker system prune -af` + `docker builder prune -af`. Keep ≥15–20 GB free.
- The `push-local.sh` script is **ASCII-only on purpose** — a non-ASCII char next to a `$VAR`
  broke it under `set -u`.
- A failed CI `deploy` run for a commit is expected (billing) — the manual deploy supersedes it.
- macOS `df /` shows the sealed system volume; check `/System/Volumes/Data` for real free space.

## Domains

`*.thesentinels.online`: `control.` (API), `gateway.` (live), the app, landing. The web app
resolves these automatically (see `sentinel-app/src/lib/config.ts`); override with
`VITE_CONTROL_URL` / `VITE_GATEWAY_URL`.
