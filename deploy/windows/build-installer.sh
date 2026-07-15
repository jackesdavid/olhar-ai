#!/usr/bin/env bash
# Assemble the Windows installer bundle (card #83, unsigned v1): a .zip you unzip on a Windows box
# and run `install.ps1` from — which installs the edge + go2rtc + ffmpeg as a real Windows service
# and opens the pairing page.
#
# This is the real installer minus two things, both deferred: a code-signing cert (so SmartScreen
# stays quiet) and an Inno Setup .exe GUI wrapper around install.ps1. The substance — a service that
# auto-starts, restarts on failure, and survives reboot — is all here.
set -euo pipefail

cd "$(dirname "$0")/../.."   # repo root
HERE=deploy/windows
OUT=$HERE/build
STAGE=$OUT/SentinelEdge

GO2RTC_URL=https://github.com/AlexxIT/go2rtc/releases/latest/download/go2rtc_win64.zip
# BtbN's static Windows ffmpeg, LGPL (not GPL) since we redistribute: a self-contained ffmpeg.exe +
# ffprobe.exe, no external DLLs.
FFMPEG_URL=https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-master-latest-win64-lgpl.zip
# WinSW v2 (.NET Framework 4.6.1+, present on every Win10/11) — tiny, wraps the console edge as a
# proper service with restart-on-failure. v3-self-contained would avoid the .NET dep at ~30 MB; not
# worth it while 4.6.1 is universal.
WINSW_URL=https://github.com/winsw/winsw/releases/download/v2.12.0/WinSW-x64.exe

VERSION=$(grep -m1 '^version' sentinel-edge/Cargo.toml | sed -E 's/.*"([^"]+)".*/\1/')
EXE=sentinel-edge/target/x86_64-pc-windows-gnu/release/sentinel-edge.exe

rm -rf "$STAGE"; mkdir -p "$STAGE" "$OUT/cache"

# 1. the edge
[ -x "$EXE" ] || { echo "build the edge first: task edge-windows-build" >&2; exit 1; }
cp "$EXE" "$STAGE/sentinel-edge.exe"

# 2. go2rtc
echo "▸ go2rtc…"
curl -fsSL --retry 3 "$GO2RTC_URL" -o "$OUT/cache/go2rtc.zip"
unzip -qo "$OUT/cache/go2rtc.zip" -d "$OUT/cache/go2rtc"
cp "$(find "$OUT/cache/go2rtc" -iname 'go2rtc*.exe' | head -1)" "$STAGE/go2rtc.exe"

# 3. ffmpeg + ffprobe
echo "▸ ffmpeg + ffprobe…"
curl -fsSL --retry 3 "$FFMPEG_URL" -o "$OUT/cache/ffmpeg.zip"
unzip -qo "$OUT/cache/ffmpeg.zip" -d "$OUT/cache/ffmpeg"
cp "$(find "$OUT/cache/ffmpeg" -iname ffmpeg.exe  | head -1)" "$STAGE/ffmpeg.exe"
cp "$(find "$OUT/cache/ffmpeg" -iname ffprobe.exe | head -1)" "$STAGE/ffprobe.exe"
# ffmpeg is redistributed here (LGPL) — its licence has to travel with it.
cp "$(find "$OUT/cache/ffmpeg" -iname 'LICENSE*' | head -1)" "$STAGE/LICENSE.ffmpeg.txt"

# 4. WinSW (the service wrapper)
echo "▸ WinSW…"
curl -fsSL --retry 3 "$WINSW_URL" -o "$STAGE/WinSW.exe"

# 5. the installer scripts + readme (from version control)
cp "$HERE/install.ps1" "$HERE/uninstall.ps1" "$HERE/README-test.txt" "$STAGE/"
mv "$STAGE/README-test.txt" "$STAGE/README.txt"

# 6. zip
ZIP="$OUT/SentinelEdge-Windows-$VERSION.zip"
rm -f "$ZIP"
( cd "$OUT" && zip -qr "$(basename "$ZIP")" SentinelEdge )

echo ""
echo "✓ $ZIP"
ls -la "$STAGE" | grep -vE '^total|^d' | awk '{printf "    %-20s %.1f MB\n", $NF, $5/1048576}'
