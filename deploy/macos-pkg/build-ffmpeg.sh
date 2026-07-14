#!/usr/bin/env bash
# Build a universal, self-contained, LGPL ffmpeg + ffprobe for the macOS appliance (card #82).
#
# Why we build our own instead of shipping a prebuilt static binary:
#
#   1. **The prebuilt ones can't be signed.** `codesign` rejects 3 of the 4 macOS binaries that
#      ffmpeg-static publishes — "main executable failed strict validation" (malformed Mach-O).
#      Notarization requires EVERY executable inside the .pkg to carry a Developer ID signature, so
#      an unsignable ffmpeg means an unnotarizable installer, which means Gatekeeper blocks the
#      download. Dead end, discovered by trying it.
#   2. **Licence certainty.** We redistribute this binary commercially. Building it ourselves with
#      `--disable-gpl --disable-nonfree` guarantees LGPL v2.1+, rather than inheriting whatever a
#      third party's CI happened to configure. (Most prebuilt macOS ffmpegs are GPL.)
#   3. **Both architectures**, from one machine.
#
# We deliberately do NOT use `--disable-everything` + a hand-picked codec list. It would produce a
# far smaller binary, but go2rtc composes its own ffmpeg command lines and a missing demuxer or
# filter would surface as a broken camera at a customer's site, not here. Size is cheap; a silent
# capability hole is not.
#
# `--disable-autodetect` is load-bearing, and it is the thing this script exists to get right.
# Without it, configure happily links whatever it finds on the BUILD machine — on a Mac with
# Homebrew that meant `/usr/local/opt/xz/lib/liblzma.5.dylib` and `/usr/local/opt/libx11/…`. Those
# paths do not exist on a customer's Mac, so the bundled ffmpeg would die on launch: precisely the
# failure the .pkg is supposed to abolish. The check at the bottom fails the build if any
# non-system dylib sneaks back in.
set -euo pipefail

cd "$(dirname "$0")"
# The release tarball, not a git clone: it's a tenth of the size, needs no git, and — unlike a
# 100 MB shallow clone — doesn't have to survive a single uninterrupted connection to succeed.
FFMPEG_VERSION=${FFMPEG_VERSION:-7.1}
OUT=$(pwd)/build/ffmpeg-universal
SRC=$(pwd)/build/ffmpeg-$FFMPEG_VERSION

command -v nasm >/dev/null || { echo "nasm is required (brew install nasm)" >&2; exit 1; }

mkdir -p "$OUT" build
if [ ! -d "$SRC" ]; then
  echo "▸ Fetching ffmpeg ${FFMPEG_VERSION}…"
  curl -fL --retry 3 --retry-delay 2 \
    "https://ffmpeg.org/releases/ffmpeg-${FFMPEG_VERSION}.tar.xz" -o "build/ffmpeg.tar.xz"
  tar -xf build/ffmpeg.tar.xz -C build
fi

build_arch() {
  arch=$1          # x86_64 | arm64
  triple=$2        # x86_64-apple-macos11 | arm64-apple-macos11
  out=$OUT/$arch
  [ -x "$out/ffmpeg" ] && { echo "▸ $arch already built"; return; }

  echo "▸ Building ffmpeg for ${arch}…"
  rm -rf "$SRC/build-$arch"; mkdir -p "$SRC/build-$arch"
  (
    cd "$SRC/build-$arch"
    ../configure \
      --prefix="$out" \
      --arch="$arch" \
      --cc="clang -arch $arch -target $triple" \
      --enable-cross-compile \
      --target-os=darwin \
      --disable-gpl --disable-nonfree \
      --disable-doc --disable-debug \
      --disable-shared --enable-static \
      --enable-videotoolbox \
      --disable-autodetect \
      >/dev/null
    make -j"$(sysctl -n hw.ncpu)" ffmpeg ffprobe >/dev/null
    mkdir -p "$out"
    cp ffmpeg ffprobe "$out/"
  )
}

build_arch x86_64 x86_64-apple-macos11
build_arch arm64  arm64-apple-macos11

echo "▸ lipo → universal"
for bin in ffmpeg ffprobe; do
  lipo -create "$OUT/x86_64/$bin" "$OUT/arm64/$bin" -output "$OUT/$bin"
done

# Two things must be true of every binary we ship, and both are cheaper to prove here than to
# discover on a customer's Mac:
#
#   1. **It links nothing but macOS itself.** A build machine with Homebrew will happily leave
#      /usr/local/opt/… in the load commands, and those paths do not exist on a clean Mac — the
#      binary would simply die on launch.
#   2. **It can be code-signed.** Notarization requires a Developer ID signature on every executable
#      in the package; an unsignable Mach-O (the reason we're not using a prebuilt ffmpeg at all)
#      would only reveal itself at the very end of a release, after the signing certs are bought.
for bin in ffmpeg ffprobe; do
  foreign=$(otool -L "$OUT/$bin" | tail -n +2 | grep -vE '^\s+(/usr/lib/|/System/)' || true)
  if [ -n "$foreign" ]; then
    echo "✗ $bin links libraries that won't exist on a customer's Mac:" >&2
    echo "$foreign" >&2
    exit 1
  fi
  codesign --force -s - "$OUT/$bin"
  echo "  $bin: $(lipo -archs "$OUT/$bin") · self-contained ✓ · signable ✓ · $(du -h "$OUT/$bin" | cut -f1)"
done

cp "$SRC/COPYING.LGPLv2.1" "$OUT/LICENSE.ffmpeg"
echo ""
echo "✓ $OUT/{ffmpeg,ffprobe} — LGPL v2.1+, universal, signable"
