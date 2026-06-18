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
- A "release" = bump web + edge together, commit, `task cloud-deploy`, and ship the edge
  binary (below). Current: **web 0.4.1, edge 0.5.0**.

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
**Linux appliance binaries:** the old static-musl `cross` path no longer works — OpenCV is a
native C++ lib that needs glibc + dynamic linking. Build on the **same Debian release** the
appliance runs (native box, container, or `cross` with a custom image) with `apt install
libopencv-dev libclang-dev clang pkg-config`, then `cargo build --release` and scp the binary
as `sentinel-edge-linux-<arch>`. The binary links OpenCV dynamically, so the run host's OpenCV
major must match the build host's (`install.sh` apt-installs the matching `libopencv-dev`).
`release.yml` needs updating from `-musl` → `-gnu` + OpenCV before CI can build Linux edges.

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
