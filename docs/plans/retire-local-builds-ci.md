# Retire Local Builds — Full CI/CD — Implementation Plan

**Board card:** Retire local builds — full CI/CD (ToDo)
**Goal:** Move every build/release off the Mac into GitHub Actions so the Mac is **coding-only**. No
build. Commit → services deploy; tag → edge binaries + installers build, host on GitHub + VPS.

## HARD BLOCKER (everything waits on this)
GitHub **Actions is billing-blocked** — "recent account payments have failed / spending limit". Pro
upgrade alone did NOT clear it. Owner must, in `github.com/settings/billing`:
1. settle the **failed payment** (valid card, no past-due), and
2. raise the **Actions spending limit** above $0.
Until Actions runs, none of this is testable. Confirmed: `deploy.yml` + `release.yml` already exist and
are correct; they just can't start.

## What already exists (verify once Actions runs)
- **`deploy.yml`** — push to `main` (path-filtered) → build 5 services (control/gateway/flow/app/landing)
  → GHCR → SSH VPS `compose pull && up -d` + sed `LATEST_EDGE_VERSION` from `sentinel-edge/Cargo.toml`.
  Secrets set: `VPS_HOST/USER/PASS`. This is "all build per commit". ✅ ready.
- **`release.yml`** — tag `v*` → build edge binary for linux x86_64/aarch64 (**static musl**, one binary
  per arch — no glibc split), macOS arm64/x86_64 → GitHub Release + SCP to VPS `/ibbf/edge-dist` (control
  serves `/edge/<asset>`, since the repo is private so Release assets aren't publicly downloadable). ✅ raw
  binaries ready.

## Phase 1 — confirm the existing pipeline (post-billing)
- `workflow_dispatch` `deploy.yml`; confirm GHCR push + VPS recreate end-to-end.
- Tag a `v0.5.x`; confirm `release.yml` builds + uploads the edge binaries.

## Phase 2 — signed macOS `.pkg` in CI (the new work)
`release.yml` builds the RAW macOS binary, not the notarized `.pkg`. Add a macOS job that runs
`deploy/macos-pkg/build.sh --sign` on a `macos-latest` runner:
- Import certs into a temp keychain from secrets (base64 `.p12` + password):
  `MACOS_APP_CERT_P12`, `MACOS_APP_CERT_PW`, `MACOS_PKG_CERT_P12`, `MACOS_PKG_CERT_PW`.
- Notarize via App Store Connect API key (not Apple-ID/password): `NOTARY_KEY_P8` (base64 of the `.p8`),
  `NOTARY_KEY_ID`, `NOTARY_ISSUER_ID` — `notarytool submit --key … --key-id … --issuer …`.
- Upload `Sentinel-Edge-<ver>.pkg` to the Release + VPS `/ibbf/edge-dist`.
- ⚠️ **Sensitive:** these are the owner's Apple signing keys + notary creds. Setting them as GH secrets is
  the owner's call (or owner-supplied values). `build.sh --sign` currently reads `MACOS_APP_CERT` /
  `MACOS_PKG_CERT` **identity names** + `NOTARY_PROFILE` — needs a small tweak to accept the CI keychain +
  API-key path. Certs today live in `~/apple-signing` (see [[apple-signing-82]]).

## Phase 3 — Windows (optional)
`edge-windows-build` exists; add a `windows`/mingw job → `SentinelEdge-Windows-<ver>.zip` → Release + VPS,
if Windows boxes are ever a thing.

## Phase 4 — reconcile the fetch path to CI artifacts
- CI Linux is **musl (static)** = ONE binary per arch. Today `install.sh` picks `-gnu` (apt) vs plain
  (dnf) and the VPS has glibc builds. When CI musl becomes the source, simplify `install.sh` to the single
  musl asset per arch and drop the glibc split ([[edge-linux-fedora-build]] becomes moot).
- Control's `installers()` endpoint already lists whatever exists for `LATEST_EDGE_VERSION` — no change.

## Version/tag flow (replaces `task edge-publish`)
- Bump `sentinel-edge/Cargo.toml`, commit, `git tag vX.Y.Z && git push --tags` → `release.yml`. Consider a
  tiny `bump+tag` helper. `deploy.yml` keeps `LATEST_EDGE_VERSION` honest from Cargo.toml on the next push.

## Mac cleanup (safe to delete ONCE CI is verified for each area)
- `cargo clean` (workspace + `sentinel-edge/`), `sentinel-edge/dist/`, `deploy/macos-pkg/build/` (the
- KEEP: `~/apple-signing` certs until they're in GH secrets AND a signed `.pkg` CI run succeeds — then the
  Mac no longer needs them.
- KEEP: the repo + toolchain for coding.

## Open decisions
1. Public downloads: keep VPS `/edge` as the public front (current), or add a **public releases repo**?
2. `.pkg` signing in CI now, or keep it the one local step (lower risk, no Apple keys in the cloud)?
3. Notary via App Store Connect **API key** (recommended for CI) — owner to create/download the `.p8`.
