#!/usr/bin/env bash
# Build the macOS **GStreamer** appliance installer for Intel: Sentinel-Edge-<version>-gst-x86_64.pkg
# (card #138). The media path is a self-contained linked GStreamer runtime — no go2rtc, no ffmpeg.
#
#   ./deploy/macos-pkg/build-gst.sh            # unsigned (dev — right-click Open, or your own Mac)
#   ./deploy/macos-pkg/build-gst.sh --sign     # signed + notarized (certs/creds as in build.sh)
#
# Intel-only on purpose: the bundled GStreamer dylibs are per-arch, the only Mac client today is
# x86_64, and a universal bundle would double the size for an arch nobody runs yet. The signing +
# notarization contract is identical to build.sh (same certs, same notary profile).
set -euo pipefail

cd "$(dirname "$0")/../.."   # repo root
HERE=deploy/macos-pkg
BUILD=$HERE/build
PAYLOAD=$BUILD/payload-gst
PREFIX=$PAYLOAD/opt/sentinel-edge

SIGN=0
[ "${1:-}" = "--sign" ] && SIGN=1

VERSION=$(grep -m1 '^version' sentinel-edge/Cargo.toml | sed -E 's/.*"([^"]+)".*/\1/')
OUT="$BUILD/Sentinel-Edge-$VERSION-gst-x86_64.pkg"

echo "▸ Sentinel Edge $VERSION (GStreamer, x86_64) → macOS .pkg"
rm -rf "$PAYLOAD" "$OUT" "$BUILD/component-gst.pkg"
mkdir -p "$PREFIX/recordings"

# ---- 1. the edge, x86_64, WITH the linked-GStreamer media path ------------
echo "▸ Building sentinel-edge (x86_64, gst-linked)…"
rustup target add x86_64-apple-darwin >/dev/null 2>&1 || true
( cd sentinel-edge && cargo build --release --features gst-linked --target x86_64-apple-darwin )
cp "sentinel-edge/target/x86_64-apple-darwin/release/sentinel-edge" "$PREFIX/sentinel-edge"

# ---- 2. the self-contained GStreamer runtime ------------------------------
# Collects the ~13 plugins our pipelines load plus their transitive Homebrew deps and rewrites every
# path to be bundle-relative — so the appliance carries its own media stack and needs no `brew`.
"$HERE/bundle-gst.sh" "$PREFIX/sentinel-edge" "$PREFIX"

echo "  edge          $(lipo -archs "$PREFIX/sentinel-edge")"

# ---- 3. sign every Mach-O -------------------------------------------------
# Notarization inspects the whole payload, so the binary AND every bundled dylib/plugin must carry a
# Developer ID signature with the hardened runtime — missing one fails notarization, not the build.
if [ "$SIGN" = 1 ]; then
  : "${MACOS_APP_CERT:?set MACOS_APP_CERT to your 'Developer ID Application: …' identity}"
  echo "▸ Signing executables + dylibs…"
  find "$PREFIX/gst" -name '*.dylib' -print0 | while IFS= read -r -d '' lib; do
    codesign --force --options runtime --timestamp -s "$MACOS_APP_CERT" "$lib"
  done
  codesign --force --options runtime --timestamp -s "$MACOS_APP_CERT" "$PREFIX/sentinel-edge"
fi

# ---- 4. build the package -------------------------------------------------
echo "▸ Building the package…"
xattr -cr "$PAYLOAD" "$HERE/scripts-gst" 2>/dev/null || true
chmod +x "$HERE/scripts-gst/postinstall"
pkgbuild \
  --root "$PAYLOAD" \
  --scripts "$HERE/scripts-gst" \
  --identifier io.sentinel.edge \
  --version "$VERSION" \
  --install-location / \
  "$BUILD/component-gst.pkg" >/dev/null

if [ "$SIGN" = 1 ]; then
  : "${MACOS_PKG_CERT:?set MACOS_PKG_CERT to your 'Developer ID Installer: …' identity}"
  productbuild --package "$BUILD/component-gst.pkg" --sign "$MACOS_PKG_CERT" "$OUT" >/dev/null
else
  productbuild --package "$BUILD/component-gst.pkg" "$OUT" >/dev/null
fi

# ---- 5. notarize + staple -------------------------------------------------
if [ "$SIGN" = 1 ]; then
  echo "▸ Notarizing (this takes a few minutes)…"
  if [ -n "${NOTARY_APPLE_ID:-}" ]; then
    xcrun notarytool submit "$OUT" \
      --apple-id "$NOTARY_APPLE_ID" --password "${NOTARY_APP_PW:?}" --team-id "${NOTARY_TEAM_ID:?}" --wait
  else
    : "${NOTARY_PROFILE:?set NOTARY_PROFILE or NOTARY_APPLE_ID/APP_PW/TEAM_ID}"
    xcrun notarytool submit "$OUT" --keychain-profile "$NOTARY_PROFILE" --wait
  fi
  xcrun stapler staple "$OUT"
  echo "▸ Gatekeeper verdict:"
  spctl -a -vvv -t install "$OUT" || true
fi

echo ""
echo "✓ $OUT ($(du -h "$OUT" | cut -f1))"
[ "$SIGN" = 0 ] && echo "  ⚠ UNSIGNED — right-click → Open on your own Mac; ship with --sign."
