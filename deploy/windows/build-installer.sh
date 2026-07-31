#!/usr/bin/env bash
# Assemble the Windows installer bundle (card #83, unsigned v1): a .zip you unzip on a Windows box
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
