#!/usr/bin/env bash
# Collect a SELF-CONTAINED GStreamer runtime next to the edge binary (card #138).
#
#   bundle-gst.sh <gst-linked-edge-binary> <bundle-dir>
#
# Why this is not `dylibbundler`: GStreamer elements live in plugin dylibs the process `dlopen`s at
# runtime, so `otool -L` on the binary never names them — a tool that only walks the binary's link
# table bundles zero plugins and the appliance fails the first time it builds a pipeline. This starts
# from the KNOWN set of plugins our pipelines instantiate, then walks every plugin's and the binary's
# transitive Homebrew deps, copies the real files, and rewrites every `/usr/local` reference to an
# `@rpath` one resolved against the binary. The result references nothing outside its own directory.
set -euo pipefail

BIN="${1:?usage: bundle-gst.sh <edge-binary> <bundle-dir>}"
OUT="${2:?usage: bundle-gst.sh <edge-binary> <bundle-dir>}"
BREW_GST="$(brew --prefix gstreamer)"
PLUGSRC="$BREW_GST/lib/gstreamer-1.0"

LIBDIR="$OUT/gst/lib"
PLUGDIR="$OUT/gst/plugins"
mkdir -p "$LIBDIR" "$PLUGDIR"

# Exactly the plugins our pipelines need. A missing one fails at RUNTIME, not here, so this list is
# the thing to update when the pipeline changes — keep it beside `capture.rs`'s pipeline string.
#   app            appsink / appsrc              rtp            rtph264depay
#   applemedia     vtdec (VideoToolbox HW)       rtsp           rtspsrc
#   coreelements   tee / queue / capsfilter      videoconvertscale  videoconvert
#   isomp4         mp4mux / qtmux                videoparsersbad    h264parse
#   jpeg           jpegenc                       videorate      videorate
#   playback       decodebin                     typefindfunctions  typefind (decodebin needs it)
#   autodetect     decodebin's sink selection
#   rtpmanager     rtspsrc's RTP session (rtpbin) — WITHOUT it rtspsrc delivers caps-less buffers
#                  and h264parse errors "No RTP format" (proven on the iOS live path, card #144)
#   udp            rtspsrc's UDP transport (udpsrc/udpsink) — the RTP/RTCP sockets
PLUGINS=(app applemedia coreelements isomp4 jpeg playback rtp rtsp \
         videoconvertscale videoparsersbad videorate typefindfunctions autodetect \
         rtpmanager udp)

echo "▸ copying ${#PLUGINS[@]} plugins"
for p in "${PLUGINS[@]}"; do
  src="$PLUGSRC/libgst$p.dylib"
  [ -f "$src" ] || { echo "  MISSING plugin: libgst$p.dylib" >&2; exit 1; }
  cp -f "$src" "$PLUGDIR/"
done

# Resolve a symlinked Homebrew dylib to its real Cellar file.
real() { python3 -c 'import os,sys; print(os.path.realpath(sys.argv[1]))' "$1"; }

# Homebrew deps of a Mach-O, one per line (skips its own id line and system libs).
brew_deps() {
  otool -L "$1" | tail -n +2 | awk '{print $1}' \
    | grep -E '^(/usr/local|/opt/homebrew)/' || true
}

echo "▸ collecting transitive dylib deps"
# Breadth-first over a worklist. Process substitution (not a pipe) keeps the copied-set in this shell.
queue=("$BIN")
for p in "$PLUGDIR"/*.dylib; do queue+=("$p"); done
i=0
while [ $i -lt ${#queue[@]} ]; do
  f="${queue[$i]}"; i=$((i + 1))
  while IFS= read -r dep; do
    [ -n "$dep" ] || continue
    base="$(basename "$dep")"
    if [ ! -f "$LIBDIR/$base" ]; then
      cp -f "$(real "$dep")" "$LIBDIR/$base"
      chmod u+w "$LIBDIR/$base"
      queue+=("$LIBDIR/$base")
    fi
  done < <(brew_deps "$f")
done
echo "  bundled $(ls "$LIBDIR" | wc -l | tr -d ' ') support dylibs"

echo "▸ rewriting install names → @rpath"
rewrite() {
  local f="$1"
  while IFS= read -r dep; do
    [ -n "$dep" ] || continue
    install_name_tool -change "$dep" "@rpath/$(basename "$dep")" "$f" 2>/dev/null || true
  done < <(brew_deps "$f")
}
for f in "$LIBDIR"/*.dylib; do
  install_name_tool -id "@rpath/$(basename "$f")" "$f" 2>/dev/null || true
  rewrite "$f"
  # Support libs sit beside each other in gst/lib and reference siblings via @rpath. Give each its
  # OWN @loader_path rpath so it resolves those siblings no matter which process loaded it — the
  # binary's rpath does NOT apply when `gst-plugin-scanner` (a separate helper) dlopen's them.
  install_name_tool -add_rpath "@loader_path" "$f" 2>/dev/null || true
done
for f in "$PLUGDIR"/*.dylib; do
  install_name_tool -id "@rpath/$(basename "$f")" "$f" 2>/dev/null || true
  rewrite "$f"
  # CRITICAL: plugins live in gst/plugins and depend on the core libs one dir up in gst/lib, via
  # @rpath. The binary's @executable_path rpath resolves those ONLY when the edge dlopen's a plugin
  # in-process — but GStreamer builds its registry in a forked `gst-plugin-scanner`, which is NOT the
  # edge binary and has no such rpath, so every plugin failed to load ("no LC_RPATH's found") and
  # `rtspsrc` never registered → live/RTSP silently broke on the bundled runtime. A plugin's own
  # @loader_path/../lib rpath makes it self-locating for ANY loader (edge, scanner, gst-launch).
  install_name_tool -add_rpath "@loader_path/../lib" "$f" 2>/dev/null || true
done
rewrite "$BIN"

# The binary also carries its own rpath so it finds libgstreamer at launch. Point it at the bundled
# lib dir; drop any inherited Homebrew rpath. `|| true` on the grep because a cargo build usually has
# NO rpath to delete (it linked absolute paths), and under `pipefail` an empty grep would otherwise
# abort the whole script right before the add_rpath.
for rp in $(otool -l "$BIN" | awk '/LC_RPATH/{r=1} r&&/path /{print $2; r=0}' \
            | grep -E '^(/usr/local|/opt/homebrew)/' || true); do
  install_name_tool -delete_rpath "$rp" "$BIN" 2>/dev/null || true
done
install_name_tool -add_rpath "@executable_path/gst/lib" "$BIN" 2>/dev/null || true

echo "▸ verifying nothing outside the bundle remains"
leak=0
for f in "$BIN" "$LIBDIR"/*.dylib "$PLUGDIR"/*.dylib; do
  if brew_deps "$f" | grep -q .; then
    echo "  LEAK in $(basename "$f"):"; brew_deps "$f" | sed 's/^/    /'
    leak=1
  fi
done
[ "$leak" = 0 ] && echo "  clean — no /usr/local or /opt/homebrew references" || { echo "  FAILED"; exit 1; }
echo "✓ self-contained GStreamer runtime in $OUT/gst ($(du -sh "$OUT/gst" | cut -f1))"
