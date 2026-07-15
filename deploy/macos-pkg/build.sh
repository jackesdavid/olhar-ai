#!/usr/bin/env bash
# Build the macOS appliance installer: Sentinel-Edge-<version>.pkg  (card #82)
#
#   ./deploy/macos-pkg/build.sh                 # unsigned — Gatekeeper WILL block it (dev only)
#   ./deploy/macos-pkg/build.sh --sign          # signed + notarized (needs the certs + creds below)
#
# A .pkg, not a .dmg: a .dmg is a disk image you drag an *app bundle* out of, and the edge is a
# headless launchd daemon — there is nothing to drag and nothing to open.
#
# The package is GENERIC: one file for every customer. It carries no box token; the appliance gets
# its identity from a pairing code typed into the page the postinstall opens (card #81). That is
# what makes a signed package possible at all — a per-box installer would have to be re-signed on
# demand, with Apple's private key sitting on a server.
#
# Signing (--sign) expects, in the environment:
#   MACOS_APP_CERT   "Developer ID Application: <Org> (TEAMID)"   → signs the executables
#   MACOS_PKG_CERT   "Developer ID Installer: <Org> (TEAMID)"     → signs the package
#   NOTARY_PROFILE   a `xcrun notarytool store-credentials` profile name
set -euo pipefail

cd "$(dirname "$0")/../.."   # repo root
HERE=deploy/macos-pkg
BUILD=$HERE/build
PAYLOAD=$BUILD/payload
PREFIX=$PAYLOAD/opt/sentinel-edge

SIGN=0
[ "${1:-}" = "--sign" ] && SIGN=1

VERSION=$(grep -m1 '^version' sentinel-edge/Cargo.toml | sed -E 's/.*"([^"]+)".*/\1/')
OUT="$BUILD/Sentinel-Edge-$VERSION.pkg"
GO2RTC_BASE=https://github.com/AlexxIT/go2rtc/releases/latest/download
# Pinned: an installer that silently changes what it ships when an upstream moves is not an
# installer, it's a surprise. Bump deliberately.
#
# ffmpeg-static publishes per-arch macOS ffmpeg AND ffprobe on GitHub releases, with the licence
# text alongside. (evermeet.cx, the usual answer, serves x86_64 only and downloads at a few hundred
# KB/s from a personal server — not something a release build should depend on.)
FFMPEG_TAG=${FFMPEG_TAG:-b6.1.1}
FFMPEG_BASE=https://github.com/eugeneware/ffmpeg-static/releases/download/$FFMPEG_TAG

echo "▸ Sentinel Edge $VERSION → macOS .pkg"
# Clear the payload, but NOT the whole build dir: ffmpeg lives under build/ffmpeg-universal and takes
# ~20 minutes to compile for two architectures. Wiping it on every run made each iteration of this
# script cost a coffee break for no reason.
rm -rf "$PAYLOAD" "$BUILD"/*.pkg
mkdir -p "$PREFIX/recordings" "$BUILD/cache" "$BUILD/dist"

# ---- 1. the edge, universal ------------------------------------------------
# Both arches from one machine. This is a dividend of card #79: with OpenCV gone the edge links
# nothing native, so `cargo build --target aarch64-apple-darwin` just works on an Intel Mac (and
# vice versa) with no cross-toolchain, no vcpkg, no dylibs to match.
echo "▸ Building sentinel-edge (x86_64 + arm64)…"
for target in x86_64-apple-darwin aarch64-apple-darwin; do
  rustup target add "$target" >/dev/null 2>&1 || true
  ( cd sentinel-edge && cargo build --release --target "$target" )
done
lipo -create \
  sentinel-edge/target/x86_64-apple-darwin/release/sentinel-edge \
  sentinel-edge/target/aarch64-apple-darwin/release/sentinel-edge \
  -output "$PREFIX/sentinel-edge"

# ---- 2. go2rtc, universal --------------------------------------------------
echo "▸ Fetching go2rtc…"
for arch in amd64 arm64; do
  curl -fsSL "$GO2RTC_BASE/go2rtc_mac_$arch.zip" -o "$BUILD/cache/go2rtc_$arch.zip"
  unzip -qo "$BUILD/cache/go2rtc_$arch.zip" -d "$BUILD/cache/go2rtc_$arch"
done
lipo -create "$BUILD/cache/go2rtc_amd64/go2rtc" "$BUILD/cache/go2rtc_arm64/go2rtc" -output "$PREFIX/go2rtc"

# ---- 3. ffmpeg + ffprobe ---------------------------------------------------
# NOT optional, and not just for snapshots: go2rtc shells out to ffmpeg to turn the camera's H264
# into the JPEG frames the motion + tracking pipeline reads. No ffmpeg, no detection. `ffprobe` is
# needed too — the edge reads a finished clip's duration with it.
#
# Bundling them is the point of the .pkg: the old installer told the customer to `brew install
# ffmpeg` and gave up if Homebrew wasn't there — a developer's package manager, on an appliance.
#
# We build them ourselves (build-ffmpeg.sh) rather than shipping someone's prebuilt static binary,
# for two reasons discovered the hard way: the popular prebuilt macOS binaries are MALFORMED and
# `codesign` refuses them ("failed strict validation"), which would make the package impossible to
# notarize; and building with --disable-gpl gives us LGPL certainty on a binary we redistribute
# commercially, instead of inheriting whatever a third party's CI configured.
FF=$HERE/build/ffmpeg-universal
if [ ! -x "$FF/ffmpeg" ] || [ ! -x "$FF/ffprobe" ]; then
  echo "  ffmpeg not built yet — running build-ffmpeg.sh (this takes a while, once)"
  "$HERE/build-ffmpeg.sh"
fi
cp "$FF/ffmpeg" "$FF/ffprobe" "$PREFIX/"
cp "$FF/LICENSE.ffmpeg" "$PREFIX/"

for bin in sentinel-edge go2rtc ffmpeg ffprobe; do
  printf "  %-14s %s\n" "$bin" "$(lipo -archs "$PREFIX/$bin")"
done

# ---- 4. sign the executables ----------------------------------------------
# Signing the PACKAGE is not enough: notarization inspects what's inside, and every Mach-O has to
# carry a Developer ID signature with the hardened runtime.
if [ "$SIGN" = 1 ]; then
  : "${MACOS_APP_CERT:?set MACOS_APP_CERT to your 'Developer ID Application: …' identity}"
  echo "▸ Signing executables…"
  # ALL FOUR — the same list the verify loop above uses. Missing one (ffprobe was omitted here once)
  # doesn't fail the build; it fails NOTARIZATION, minutes later, with "not signed with a valid
  # Developer ID certificate" for exactly the binary left out. Keep this list identical to line ~92.
  for bin in sentinel-edge go2rtc ffmpeg ffprobe; do
    codesign --force --options runtime --timestamp -s "$MACOS_APP_CERT" "$PREFIX/$bin"
  done
fi

# ---- 5. build the package --------------------------------------------------
# Strip what extended attributes we can: pkgbuild preserves them as AppleDouble `._` side-files, so
# they end up as clutter next to every binary in the payload.
#
# `com.apple.provenance` will survive this, and that is not a bug in the script: since macOS Sequoia
# it is a system-protected attribute that a normal user cannot clear — the OS re-applies it the
# moment it's removed. The resulting `._` files are inert (they are not executables, the notary
# ignores them) and this is what Apple's own tooling produces. Left alone deliberately, rather than
# chased.
echo "▸ Building the package…"
xattr -cr "$PAYLOAD" "$HERE/scripts" 2>/dev/null || true
chmod +x "$HERE/scripts/postinstall"
pkgbuild \
  --root "$PAYLOAD" \
  --scripts "$HERE/scripts" \
  --identifier io.sentinel.edge \
  --version "$VERSION" \
  --install-location / \
  "$BUILD/component.pkg" >/dev/null

if [ "$SIGN" = 1 ]; then
  : "${MACOS_PKG_CERT:?set MACOS_PKG_CERT to your 'Developer ID Installer: …' identity}"
  productbuild --package "$BUILD/component.pkg" --sign "$MACOS_PKG_CERT" "$OUT" >/dev/null
else
  productbuild --package "$BUILD/component.pkg" "$OUT" >/dev/null
fi

# ---- 6. notarize + staple --------------------------------------------------
# Without this, Gatekeeper refuses to open the package: "cannot be opened because it is from an
# unidentified developer." That is a WORSE experience than the `curl | sh` this replaces, which is
# why an unsigned build is a dev artifact, not a release. Stapling attaches the notarization ticket
# so the check works on a machine that's offline.
if [ "$SIGN" = 1 ]; then
  : "${NOTARY_PROFILE:?set NOTARY_PROFILE (xcrun notarytool store-credentials …)}"
  echo "▸ Notarizing (this takes a few minutes)…"
  xcrun notarytool submit "$OUT" --keychain-profile "$NOTARY_PROFILE" --wait
  xcrun stapler staple "$OUT"
  echo "▸ Gatekeeper verdict:"
  spctl -a -vvv -t install "$OUT" || true
fi

cp "$OUT" "$BUILD/dist/"
echo ""
echo "✓ $OUT ($(du -h "$OUT" | cut -f1))"
if [ "$SIGN" = 0 ]; then
  echo ""
  echo "  ⚠ UNSIGNED. Gatekeeper will refuse to open this on any Mac but your own"
  echo "    (right-click → Open bypasses it, for testing). Ship with --sign."
fi
