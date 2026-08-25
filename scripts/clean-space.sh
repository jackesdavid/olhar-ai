#!/usr/bin/env bash
# Reclaim disk space, and say where the rest of it went.
#
# Card #197. The previous version cleaned `target/` and Docker, which on this machine is under a
# gigabyte, while 32 GB sat in caches it never looked at. That is worse than doing nothing: it
# reports success, frees nothing that matters, and the disk fills again the same afternoon.
#
# This machine is also the CI runner. When the disk fills, jobs do not fail with a compile error
# — the Docker daemon dies and every job reports `system failure`, which reads like a flake and
# is not one.
#
# Everything removed here is a build artifact or a cache that regenerates on demand. What is
# NOT removed, and why, is at the bottom — that list is the point of the card.

set -uo pipefail

human() { du -sh "$1" 2>/dev/null | cut -f1; }
free_now() { df -h / | awk 'NR==2 {print $4}'; }

before_kb=$(df -k / | awk 'NR==2 {print $4}')
echo "free before: $(free_now)"
echo

drop() {
  local label="$1" path="$2"
  [ -e "$path" ] || return 0
  local size
  size=$(human "$path")
  printf '  %-28s %6s  %s\n' "$label" "${size:-?}" "$path"
  rm -rf "$path"
}

run() {
  local label="$1"; shift
  printf '  %-28s        %s\n' "$label" "$*"
  "$@" >/dev/null 2>&1 || true
}

echo "Rust"
run "workspace target"        cargo clean
run "edge target"             cargo clean --manifest-path sentinel-edge/Cargo.toml
# The registry re-downloads on the next build. Keep `registry/index`: dropping it costs a full
# index refresh for no meaningful space.
drop "crate sources"          "$HOME/.cargo/registry/src"
drop "crate archives"         "$HOME/.cargo/registry/cache"

echo
echo "Installers"
drop "macOS pkg build"        "deploy/macos-pkg/build"
drop "Windows build"          "deploy/windows/build"

echo
echo "Xcode"
# By far the biggest single item on a machine that has ever debugged on a phone: Xcode keeps a
# full symbol set per iOS build of per device. It re-extracts on the next connect — a few
# minutes, once.
drop "iOS device symbols"     "$HOME/Library/Developer/Xcode/iOS DeviceSupport"
drop "derived data"           "$HOME/Library/Developer/Xcode/DerivedData"
drop "preview caches"         "$HOME/Library/Developer/Xcode/UserData/Previews"
# Simulator runtimes that no installed Xcode can boot any more.
run "unbootable simulators"   xcrun simctl delete unavailable

echo
echo "Language caches"
run "Go module cache"         go clean -modcache
run "npm cache"               npm cache clean --force
run "Homebrew downloads"      brew cleanup -s --prune=all

echo
echo "Docker"
# Unused images. NOT `system prune`, which takes the BUILD CACHE with them — and this machine is
# the CI runner, so a cold cargo-chef layer is paid back at about twenty minutes per Rust build.
# Measured: `build:rust` went 24 → 43 minutes across runs where this had wiped the cache, and
# `docker system df` reported `Build Cache 0B` afterwards every time.
run "unused images"           docker image prune -af
# The cache itself, trimmed rather than emptied: a week is long enough that an ordinary week of
# commits keeps hitting it, and short enough that a branch nobody touched since is not being
# stored for free.
run "build cache over a week" docker builder prune -f --filter until=168h
# NOT `--volumes` anywhere above: those hold the local Postgres from `task db-up`, and losing a
# dev database to a disk-cleanup command is a bad trade.

echo
after_kb=$(df -k / | awk 'NR==2 {print $4}')
freed_gb=$(awk -v a="$after_kb" -v b="$before_kb" 'BEGIN { printf "%.1f", (a-b)/1048576 }')
echo "reclaimed ${freed_gb}G — free now: $(free_now)"

echo
echo "Left alone, on purpose:"
printf '  %-28s %6s  %s\n' "Xcode archives" "$(human "$HOME/Library/Developer/Xcode/Archives")" \
  "shipped builds — needed to symbolicate a crash or resubmit"
printf '  %-28s %6s  %s\n' "simulator devices" "$(human "$HOME/Library/Developer/CoreSimulator/Devices")" \
  "installed runtimes + their data; delete from Xcode if you want them gone"
printf '  %-28s %6s  %s\n' "CI runner workspaces" "$(human "$HOME/builds")" \
  "the runner reuses these; clearing them mid-job kills the job"
printf '  %-28s %6s  %s\n' "Docker volumes" "-" \
  "the local Postgres lives here — 'docker volume prune' if you truly mean it"
echo
echo "Biggest directories still on disk:"
du -sh "$HOME"/Library/* "$HOME"/* 2>/dev/null | sort -rh | head -8 | sed 's/^/  /'
