#!/usr/bin/env bash
# Build the embedded edge static library for iOS device and drop it where the pod expects it.
# Run before `npx expo prebuild` / a device build. The .a is a build artefact — it is gitignored.
#
#   ./modules/sentinel-edge/build-ios-lib.sh
#
# GStreamer is REQUIRED, not optional. This script used to gate it behind `GST=1` and
# `--features gst-linked`, but 277f079 deleted that feature: GStreamer is the only media path, so a
# flag to select it selected nothing. Left as it was, the default invocation failed with
# "the package 'sentinel-edge' does not contain this feature: gst-linked" and the GST=1 form was the
# only one that worked — the opposite of what the usage text promised.
#
# Needs the GStreamer iOS SDK at ~/Library/Developer/GStreamer/iPhone.sdk.
set -euo pipefail

cd "$(dirname "$0")/../../.."   # repo root
EDGE=sentinel-edge
OUT="sentinel-mobile/modules/sentinel-edge/ios/libs/libsentinel_edge.a"

# Without the target, cargo fails deep in a dependency with "can't find crate for `core`" — which
# reads as a broken dependency rather than a missing toolchain component. Say what it is.
if ! rustup target list --installed | grep -q '^aarch64-apple-ios$'; then
  echo "Rust target aarch64-apple-ios is not installed. Run:" >&2
  echo "  rustup target add aarch64-apple-ios" >&2
  exit 1
fi

FWPARENT="$HOME/Library/Developer/GStreamer/iPhone.sdk"
if [ ! -d "$FWPARENT" ]; then
  echo "GStreamer iOS SDK not found at $FWPARENT" >&2
  echo "Install it from https://gstreamer.freedesktop.org/download/ (iOS runtime + devel)." >&2
  exit 1
fi

# Synthetic pkg-config for the monolithic iOS GStreamer framework (#144). The SDK ships one
# framework rather than the per-library .pc files the crates' build scripts look for, so point every
# name they ask about at the same place.
PCDIR="$(mktemp -d)"
FW="$FWPARENT/GStreamer.framework/Versions/Current"
write_pc() {
  printf 'prefix=%s\nName: %s\nVersion: %s\nCflags: -I%s/Headers\nLibs: -F%s -framework GStreamer\n' \
    "$FW" "$1" "$2" "$FW" "$FWPARENT" > "$PCDIR/$1.pc"
}
for p in glib-2.0 gobject-2.0 gio-2.0 gmodule-2.0 gmodule-no-export-2.0; do write_pc "$p" 2.80.0; done
for p in gstreamer-1.0 gstreamer-base-1.0 gstreamer-app-1.0; do write_pc "$p" 1.28.5; done
export PKG_CONFIG_PATH="$PCDIR" PKG_CONFIG_ALLOW_CROSS=1
export RUSTFLAGS="-L framework=$FWPARENT -l framework=GStreamer"

( cd "$EDGE" && cargo build --release --lib --target aarch64-apple-ios )
cp "$EDGE/target/aarch64-apple-ios/release/libsentinel_edge.a" "$OUT"

# The pod links this by name; a stale .a that silently lacks a symbol the Swift side calls fails at
# LINK time with an undefined symbol, which is confusing to read as "the feature does not work".
# Assert the FFI surface is present instead.
#
# `nm` exits non-zero when any archive member carries no symbol table, which most of them do. Under
# `set -o pipefail` that sinks the whole pipeline before grep is consulted, so the check reported
# every symbol missing while all three were present. Capture the output once, then search it.
# Via a temp FILE, not a shell variable: this archive is ~155 MB and its symbol list is far past
# what a command-line argument can carry, so `printf '%s' "$SYMS" | grep` fails on length rather
# than on content — reporting every symbol missing while all three are present.
SYMS="$(mktemp)"
trap 'rm -f "$SYMS"' EXIT
nm "$OUT" > "$SYMS" 2>/dev/null || true
for sym in start_edge stop_edge push_camera_frame; do
  if ! grep -qE "^[0-9a-f]+ T _$sym\$" "$SYMS"; then
    echo "✗ $OUT does not export $sym — the Swift bridge would fail to link" >&2
    exit 1
  fi
done

echo "✓ $OUT ($(du -h "$OUT" | cut -f1)) — exports start_edge, stop_edge, push_camera_frame"
