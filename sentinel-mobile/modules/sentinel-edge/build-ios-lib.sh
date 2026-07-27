#!/usr/bin/env bash
# Build the embedded edge static library for iOS device and drop it where the pod expects it.
# Run before `npx expo prebuild` / a device build. The .a is a build artefact — it is gitignored.
#
#   ./modules/sentinel-edge/build-ios-lib.sh            # pure-Rust edge (no GStreamer) — boot proof
#   GST=1 ./modules/sentinel-edge/build-ios-lib.sh      # + gst-linked (needs the GStreamer iOS SDK)
set -euo pipefail
write_pc() { printf 'prefix=%s\nName: %s\nVersion: %s\nCflags: -I%s/Headers\nLibs: -F%s -framework GStreamer\n' \
  "$FW" "$p" "$V" "$FW" "$FWPARENT" > "$PCDIR/$p.pc"; }
cd "$(dirname "$0")/../../.."   # repo root
EDGE=sentinel-edge
OUT="sentinel-mobile/modules/sentinel-edge/ios/libs/libsentinel_edge.a"

FEATURES=""
EXTRA_ENV=()
if [ "${GST:-0}" = "1" ]; then
  FEATURES="--features gst-linked"
  PCDIR="$(mktemp -d)"   # synthetic pkg-config for the monolithic iOS GStreamer framework (#144)
  FW="$HOME/Library/Developer/GStreamer/iPhone.sdk/GStreamer.framework/Versions/Current"
  FWPARENT="$HOME/Library/Developer/GStreamer/iPhone.sdk"
  for p in glib-2.0 gobject-2.0 gio-2.0 gmodule-2.0 gmodule-no-export-2.0; do V=2.80.0; write_pc; done
  for p in gstreamer-1.0 gstreamer-base-1.0 gstreamer-app-1.0; do V=1.28.5; write_pc; done
  export PKG_CONFIG_PATH="$PCDIR" PKG_CONFIG_ALLOW_CROSS=1
  export RUSTFLAGS="-L framework=$FWPARENT -l framework=GStreamer"
fi

( cd "$EDGE" && cargo build --release --lib --target aarch64-apple-ios $FEATURES )
cp "$EDGE/target/aarch64-apple-ios/release/libsentinel_edge.a" "$OUT"
echo "✓ $OUT ($(du -h "$OUT" | cut -f1))"
