#!/usr/bin/env bash
# Fetch the two binary artefacts an iOS build needs but git does not carry (#185).
#
# WHY THIS EXISTS. The embedded edge (#144) links two things that are far too large for a
# repository and are produced by toolchains a CI worker does not have:
#
#   libsentinel_edge.a   155 MB  — a Rust cross-compile for aarch64-apple-ios
#   GStreamer.framework  1.3 GB  — a manual SDK install under a developer's $HOME
#
# Both are gitignored, and EAS honours .gitignore when packaging, so neither reached the builder.
# The build compiled everything and then died with "linker command failed" — the real cause,
# `framework 'GStreamer' not found`, was buried thousands of lines into a brotli-compressed log.
#
# Building them ON the worker is not an option: that means a Rust toolchain plus a 2.5 GB SDK
# install on every build. So both are published once to the VPS — which already serves edge
# binaries from /ibbf/edge-dist through control's `/edge/<asset>` — and pulled here.
#
# ⚠️  THE STALENESS TRAP. Nothing ties these artefacts to the sources that produced them. A build
#     would happily link a months-old edge and ship it, silently. The checksums below are the
#     guard: verified after download, so a mismatch fails LOUDLY instead of shipping code nobody
#     chose. Update them in the same commit that republishes an artefact — the pair is the
#     contract, exactly like TERMS_VERSION and the legal documents.
set -euo pipefail

BASE="${EDGE_DIST_URL:-https://control.thesentinels.online/edge}"
LIB_SHA="e504a9169d660d83"   # libsentinel_edge-ios-arm64.a      — published 2026-08-16
GST_SHA="0ec5b940ecb082ff"   # gstreamer-ios-arm64.tar.gz        — published 2026-08-16

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
LIBS="$ROOT/modules/sentinel-edge/ios/libs"
mkdir -p "$LIBS"

sha16() { shasum -a 256 "$1" | cut -c1-16; }

# --- the edge static library ------------------------------------------------------------------
if [ -f "$LIBS/libsentinel_edge.a" ]; then
  echo "edge lib already present — leaving the local build alone"
else
  echo "fetching libsentinel_edge.a (155 MB)"
  curl -fSL --retry 3 --retry-delay 2 -o "$LIBS/libsentinel_edge.a" "$BASE/libsentinel_edge-ios-arm64.a"
  got="$(sha16 "$LIBS/libsentinel_edge.a")"
  if [ "$got" != "$LIB_SHA" ]; then
    echo "✗ edge lib checksum mismatch: expected $LIB_SHA, got $got" >&2
    rm -f "$LIBS/libsentinel_edge.a"; exit 1
  fi
  echo "✓ edge lib verified"
fi

# --- the GStreamer framework ------------------------------------------------------------------
# A developer Mac has the real SDK under $HOME and the podspec falls back to it, so this only runs
# where the framework is genuinely absent — a fresh worker.
if [ -d "$LIBS/GStreamer.framework" ]; then
  echo "GStreamer.framework already present"
elif [ -d "$HOME/Library/Developer/GStreamer/iPhone.sdk/GStreamer.framework" ]; then
  echo "GStreamer SDK found under \$HOME — the podspec falls back to it, nothing to fetch"
else
  echo "fetching GStreamer.framework (392 MB compressed, 1.3 GB unpacked)"
  tmp="$(mktemp -d)"
  curl -fSL --retry 3 --retry-delay 2 -o "$tmp/gst.tar.gz" "$BASE/gstreamer-ios-arm64.tar.gz"
  got="$(sha16 "$tmp/gst.tar.gz")"
  if [ "$got" != "$GST_SHA" ]; then
    echo "✗ GStreamer checksum mismatch: expected $GST_SHA, got $got" >&2
    rm -rf "$tmp"; exit 1
  fi
  tar -C "$LIBS" -xzf "$tmp/gst.tar.gz"
  rm -rf "$tmp"
  echo "✓ GStreamer.framework verified and unpacked"
fi
