#!/usr/bin/env sh
# sentinel-edge — one-command appliance installer.
#
# A client runs this ONCE on their on-prem box. It:
#   1. installs Tailscale (if missing),
#   2. downloads the sentinel-edge + go2rtc binaries,
#   3. writes the config, joins the tailnet, and serves go2rtc over it,
#   4. installs a service (systemd / launchd) so it runs 24/7 and on boot,
#   5. starts it.
#
# The web app (Settings → Edge boxes) hands the client a ready-made command with the
# box token + secrets filled in:
#
#   curl -fsSL https://control.thesentinels.online/install.sh | sudo sh -s -- \
#     --control https://control.thesentinels.online \
#     --box-token sbx_xxxxxxxx \
#     --authkey tskey-auth-xxxx \
#     --hostname tenant-001
#
# The box token binds this appliance to its tenant: control returns only that tenant's
# cameras. The Tailscale auth key joins the tailnet so the gateway can reach go2rtc.
set -eu

# ---- config (flags > env > default) ----------------------------------------
AUTHKEY="${SENTINEL_TS_AUTHKEY:-}"
HOSTNAME_="${SENTINEL_HOSTNAME:-sentinel-edge}"
TAGS="${SENTINEL_TS_TAGS:-}"
CONTROL_URL="${SENTINEL_CONTROL_URL:-}"
BOX_TOKEN="${SENTINEL_BOX_TOKEN:-}"
PUBLIC_HOST="${SENTINEL_PUBLIC_HOST:-}"
# Where to fetch the sentinel-edge binary. Defaults to the control plane (the repo is
# private, so control serves the binaries at /edge/<asset>); override if self-hosting.
RELEASE_BASE="${SENTINEL_RELEASE_BASE:-}"
GO2RTC_BASE="${SENTINEL_GO2RTC_BASE:-https://github.com/AlexxIT/go2rtc/releases/latest/download}"
PREFIX="${SENTINEL_PREFIX:-/opt/sentinel-edge}"
ENV_DIR="${SENTINEL_ENV_DIR:-/etc/sentinel-edge}"
UPDATE=0   # --update: only swap the binary + restart (keep env, Tailscale, service)

while [ $# -gt 0 ]; do
  case "$1" in
    --authkey) AUTHKEY="$2"; shift 2 ;;
    --hostname) HOSTNAME_="$2"; shift 2 ;;
    --tags) TAGS="$2"; shift 2 ;;
    --control) CONTROL_URL="$2"; shift 2 ;;
    --box-token) BOX_TOKEN="$2"; shift 2 ;;
    --public-host) PUBLIC_HOST="$2"; shift 2 ;;
    --release-base) RELEASE_BASE="$2"; shift 2 ;;
    --update) UPDATE=1; shift ;;
    *) echo "unknown flag: $1" >&2; exit 1 ;;
  esac
done

[ "$(id -u)" -eq 0 ] || { echo "Please run as root (sudo)." >&2; exit 1; }
# In update mode, inherit the control URL from the existing install if not passed.
if [ "$UPDATE" = "1" ] && [ -z "$CONTROL_URL" ] && [ -f "$ENV_DIR/edge.env" ]; then
  CONTROL_URL="$(grep '^CONTROL_URL=' "$ENV_DIR/edge.env" | cut -d= -f2-)"
fi
[ -n "$CONTROL_URL" ] || { echo "Missing --control <url> (the control-plane base URL)." >&2; exit 1; }
[ "$UPDATE" = "1" ] || [ -n "$BOX_TOKEN" ] || echo "Note: no --box-token given; this edge will use the default tenant." >&2
# Default the binary source to the control plane (serves them at /edge/<asset>).
[ -n "$RELEASE_BASE" ] || RELEASE_BASE="$CONTROL_URL/edge"

# ---- detect platform --------------------------------------------------------
os="$(uname -s)"; arch="$(uname -m)"
case "$os" in
  Linux)  EDGE_ASSET="sentinel-edge-linux-$arch" ;;
  Darwin) EDGE_ASSET="sentinel-edge-macos-$arch" ;;
  *) echo "Unsupported OS: $os" >&2; exit 1 ;;
esac
case "$os-$arch" in
  Linux-x86_64)   GO2RTC_ASSET="go2rtc_linux_amd64" ;;
  Linux-aarch64)  GO2RTC_ASSET="go2rtc_linux_arm64" ;;
  Darwin-arm64)   GO2RTC_ASSET="go2rtc_mac_arm64.zip" ;;
  Darwin-x86_64)  GO2RTC_ASSET="go2rtc_mac_amd64.zip" ;;
  *) echo "Unsupported platform: $os-$arch" >&2; exit 1 ;;
esac

# macOS: ensure the open-source tailscaled is registered as a launchd system daemon AND
# actually running, so `tailscale up` can reach it headlessly. Idempotent — safe to re-run.
# (Linux's installer wires up tailscaled itself.) A binary-only `--update` can leave a box
# without it (that's how a box ends up "offline on the tailnet"), so BOTH paths call this.
ensure_tailscaled_macos() {
  [ "$os" = "Darwin" ] || return 0
  TAILSCALED="$(command -v tailscaled || echo /usr/local/bin/tailscaled)"
  echo "▸ Ensuring tailscaled system daemon…"
  "$TAILSCALED" install-system-daemon 2>&1 \
    || echo "  ⚠ 'tailscaled install-system-daemon' failed — the box will be off the tailnet (Live won't work)." >&2
  # Wait for the daemon to come up so a 'tailscale up' right after can reach it.
  i=0
  while [ "$i" -lt 10 ] && ! pgrep -x tailscaled >/dev/null 2>&1; do sleep 1; i=$((i+1)); done
  if pgrep -x tailscaled >/dev/null 2>&1; then
    echo "✓ tailscaled running"
  else
    echo "  ⚠ tailscaled not running after install — Live will be offline until it is. Re-run: sudo $TAILSCALED install-system-daemon" >&2
  fi
}

# ---- update mode: swap the binary + restart; keep env / Tailscale / service ---
if [ "$UPDATE" = "1" ]; then
  [ -x "$PREFIX/sentinel-edge" ] || { echo "Not installed at $PREFIX — run the full installer first." >&2; exit 1; }
  echo "▸ Updating sentinel-edge ($os-$arch) from $RELEASE_BASE…"
  curl -fSL "$RELEASE_BASE/$EDGE_ASSET" -o "$PREFIX/sentinel-edge.new"
  chmod +x "$PREFIX/sentinel-edge.new"
  mv "$PREFIX/sentinel-edge.new" "$PREFIX/sentinel-edge"
  if [ "$os" = "Linux" ]; then
    systemctl restart sentinel-edge
    echo "✓ Updated. Status: systemctl status sentinel-edge"
  else
    # A binary-only update can have lost the tailscaled daemon — re-assert it before the
    # restart so the box rejoins the tailnet (the edge's ensure_up runs `tailscale up`).
    ensure_tailscaled_macos
    launchctl kickstart -k system/io.sentinel.edge 2>/dev/null \
      || { launchctl bootout system /Library/LaunchDaemons/io.sentinel.edge.plist 2>/dev/null; \
           launchctl bootstrap system /Library/LaunchDaemons/io.sentinel.edge.plist; }
    echo "✓ Updated."
  fi
  exit 0
fi

echo "▸ Installing sentinel-edge ($os-$arch) → $PREFIX"
mkdir -p "$PREFIX" "$ENV_DIR"

# ---- 1. Tailscale -----------------------------------------------------------
# A working `tailscale version` is the real test (a leftover GUI-app shim resolves on PATH
# but is broken once the app is gone).
if ! tailscale version >/dev/null 2>&1; then
  echo "▸ Installing Tailscale…"
  if [ "$os" = "Linux" ]; then
    curl -fsSL https://tailscale.com/install.sh | sh
  else
    # macOS: headless install — the open-source tailscaled (NOT the GUI app), so the edge
    # joins the tailnet from the CLI exactly like a Linux appliance. Homebrew refuses root,
    # so install as the invoking user; drop any stale GUI-app shim first.
    rm -f /usr/local/bin/tailscale 2>/dev/null || true
    # Non-interactive: skip the slow formula auto-update + the tap-trust prompt (either can
    # hang/abort a headless install on a box with extra taps).
    BREW_ENV="HOMEBREW_NO_AUTO_UPDATE=1 HOMEBREW_NO_REQUIRE_TAP_TRUST=1 HOMEBREW_NO_ENV_HINTS=1"
    if [ -n "${SUDO_USER:-}" ] && sudo -u "$SUDO_USER" command -v brew >/dev/null 2>&1; then
      sudo -u "$SUDO_USER" env $BREW_ENV brew install tailscale && sudo -u "$SUDO_USER" env $BREW_ENV brew link --overwrite tailscale
    elif command -v brew >/dev/null 2>&1; then
      env $BREW_ENV brew install tailscale && env $BREW_ENV brew link --overwrite tailscale
    else
      echo "  Homebrew not found — install it (https://brew.sh), or Tailscale, then re-run." >&2
      exit 1
    fi
  fi
fi

# macOS: register + start the tailscaled system daemon (launchd) so `tailscale up` works
# headlessly. (Linux's installer already sets up tailscaled.)
ensure_tailscaled_macos

# Join the tailnet now (best-effort) so the box is reachable the moment install finishes;
# the edge service also re-asserts this on boot. Needs a key (auto-minted or --authkey).
if [ -n "$AUTHKEY" ]; then
  echo "▸ Joining the tailnet (hostname=$HOSTNAME_)…"
  # shellcheck disable=SC2086
  tailscale up --authkey="$AUTHKEY" --hostname="$HOSTNAME_" ${TAGS:+--advertise-tags="$TAGS"} \
    || echo "  'tailscale up' failed — check the auth key; the edge service will retry on boot." >&2
fi

# ---- 1b. ffmpeg -------------------------------------------------------------
# go2rtc shells out to ffmpeg to produce JPEG frames (the Live snapshot + the
# GetCurrentFrame workflow node hit /api/frame.jpeg). Without it those fail with
# `exec: "ffmpeg": executable file not found in $PATH` while MSE/WebRTC still plays.
if ! command -v ffmpeg >/dev/null 2>&1; then
  echo "▸ Installing ffmpeg…"
  if [ "$os" = "Linux" ]; then
    if command -v apt-get >/dev/null 2>&1; then
      apt-get update -qq && apt-get install -y ffmpeg
    elif command -v dnf >/dev/null 2>&1; then
      dnf install -y ffmpeg
    elif command -v yum >/dev/null 2>&1; then
      yum install -y ffmpeg
    elif command -v apk >/dev/null 2>&1; then
      apk add --no-cache ffmpeg
    else
      echo "  No supported package manager found — install ffmpeg manually, then re-run." >&2
    fi
  else
    # macOS: Homebrew refuses to run as root, so install as the invoking user.
    if [ -n "${SUDO_USER:-}" ] && command -v sudo >/dev/null 2>&1 && sudo -u "$SUDO_USER" command -v brew >/dev/null 2>&1; then
      sudo -u "$SUDO_USER" brew install ffmpeg || true
    elif command -v brew >/dev/null 2>&1; then
      brew install ffmpeg || true
    else
      echo "  Install ffmpeg first:  brew install ffmpeg  — then re-run." >&2
    fi
  fi
fi
if command -v ffmpeg >/dev/null 2>&1; then
  echo "✓ ffmpeg present ($(command -v ffmpeg))"
else
  echo "⚠ ffmpeg NOT found — live snapshots / GetCurrentFrame will fail until it is installed." >&2
fi

# ---- 1c. OpenCV (continuous motion detection) -------------------------------
# The edge runs a per-camera MOG2 motion watcher (links OpenCV). Required — the runtime
# OpenCV major must match the build host's (the binary links dynamically); on a self-built
# box (build + run on the same machine) they match by construction.
echo "▸ Installing OpenCV (motion detection)…"
if [ "$os" = "Linux" ]; then
  if command -v apt-get >/dev/null 2>&1; then
    apt-get update -qq && apt-get install -y libopencv-dev
  elif command -v dnf >/dev/null 2>&1; then
    dnf install -y opencv opencv-devel
  elif command -v apk >/dev/null 2>&1; then
    apk add --no-cache opencv
  else
    echo "  No supported package manager found — install OpenCV manually, then re-run." >&2
  fi
else
  if [ -n "${SUDO_USER:-}" ] && command -v sudo >/dev/null 2>&1 && sudo -u "$SUDO_USER" command -v brew >/dev/null 2>&1; then
    sudo -u "$SUDO_USER" brew install opencv || true
  elif command -v brew >/dev/null 2>&1; then
    brew install opencv || true
  else
    echo "  Install OpenCV first:  brew install opencv  — then re-run." >&2
  fi
fi
if command -v pkg-config >/dev/null 2>&1 && pkg-config --exists opencv4 2>/dev/null; then
  echo "✓ OpenCV present ($(pkg-config --modversion opencv4))"
else
  echo "⚠ OpenCV not detected via pkg-config — the motion watcher will fail to start until it is installed." >&2
fi

# ---- 2. binaries ------------------------------------------------------------
echo "▸ Downloading sentinel-edge…"
curl -fSL "$RELEASE_BASE/$EDGE_ASSET" -o "$PREFIX/sentinel-edge"
chmod +x "$PREFIX/sentinel-edge"

echo "▸ Downloading go2rtc…"
case "$GO2RTC_ASSET" in
  *.zip)
    tmp="$(mktemp -d)"
    curl -fSL "$GO2RTC_BASE/$GO2RTC_ASSET" -o "$tmp/go2rtc.zip"
    unzip -o "$tmp/go2rtc.zip" -d "$tmp" >/dev/null
    mv "$(find "$tmp" -type f ! -name '*.zip' | head -1)" "$PREFIX/go2rtc"
    rm -rf "$tmp" ;;
  *)
    curl -fSL "$GO2RTC_BASE/$GO2RTC_ASSET" -o "$PREFIX/go2rtc" ;;
esac
chmod +x "$PREFIX/go2rtc"

# ---- 3. config --------------------------------------------------------------
# Bake a PATH so the service (and the go2rtc child it spawns) can find ffmpeg + tailscale
# even under a minimal systemd/launchd PATH (Homebrew dirs are NOT on a daemon's default).
EDGE_PATH="/usr/local/bin:/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin"
FFMPEG_BIN="$(command -v ffmpeg 2>/dev/null || true)"
[ -n "$FFMPEG_BIN" ] && EDGE_PATH="$(dirname "$FFMPEG_BIN"):$EDGE_PATH"

echo "▸ Writing $ENV_DIR/edge.env"
cat > "$ENV_DIR/edge.env" <<EOF
# Generated by install.sh — sentinel-edge appliance config.
EDGE_BIND=127.0.0.1:8788
CONTROL_URL=$CONTROL_URL
BOX_TOKEN=$BOX_TOKEN
EDGE_PUBLIC_HOST=$PUBLIC_HOST
EDGE_OWN_GO2RTC=1
GO2RTC_BIN=$PREFIX/go2rtc
GO2RTC_CONFIG=$PREFIX/go2rtc.yaml
EDGE_CACHE=$PREFIX/cameras-cache.json
TS_AUTHKEY=$AUTHKEY
TS_HOSTNAME=$HOSTNAME_
TS_TAGS=$TAGS
PATH=$EDGE_PATH
EOF
chmod 600 "$ENV_DIR/edge.env"   # contains the auth key + box token

# ---- 4. service (the edge self-enrolls Tailscale + serve on startup) --------
if [ "$os" = "Linux" ]; then
  echo "▸ Installing systemd service…"
  cat > /etc/systemd/system/sentinel-edge.service <<EOF
[Unit]
Description=Sentinel edge gateway (cameras + go2rtc + tailnet)
After=network-online.target tailscaled.service
Wants=network-online.target

[Service]
EnvironmentFile=$ENV_DIR/edge.env
ExecStart=$PREFIX/sentinel-edge
Restart=always
RestartSec=3

[Install]
WantedBy=multi-user.target
EOF
  systemctl daemon-reload
  systemctl enable --now sentinel-edge
  echo "✓ Installed. Status:  systemctl status sentinel-edge"
else
  echo "▸ Installing launchd agent…"
  PLIST=/Library/LaunchDaemons/io.sentinel.edge.plist
  # shellcheck disable=SC2016
  {
    printf '%s\n' '<?xml version="1.0" encoding="UTF-8"?>'
    printf '%s\n' '<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">'
    printf '%s\n' '<plist version="1.0"><dict>'
    printf '%s\n' '  <key>Label</key><string>io.sentinel.edge</string>'
    printf '%s\n' "  <key>ProgramArguments</key><array><string>$PREFIX/sentinel-edge</string></array>"
    printf '%s\n' '  <key>EnvironmentVariables</key><dict>'
    while IFS='=' read -r k v; do
      case "$k" in ''|\#*) continue ;; esac
      printf '    <key>%s</key><string>%s</string>\n' "$k" "$v"
    done < "$ENV_DIR/edge.env"
    printf '%s\n' '  </dict>'
    printf '%s\n' '  <key>RunAtLoad</key><true/><key>KeepAlive</key><true/>'
    printf '%s\n' '</dict></plist>'
  } > "$PLIST"
  # Re-bootstrap cleanly: boot out any prior instance first, or a fresh install over an
  # existing service fails with "Load failed: 5: Input/output error". `load` is deprecated.
  launchctl bootout system "$PLIST" 2>/dev/null || true
  if ! launchctl bootstrap system "$PLIST"; then
    echo "  launchctl bootstrap failed — retrying after bootout…" >&2
    launchctl bootout system "$PLIST" 2>/dev/null || true
    launchctl bootstrap system "$PLIST" || echo "  Still failing — run: sudo launchctl bootstrap system $PLIST" >&2
  fi
  launchctl kickstart -k system/io.sentinel.edge 2>/dev/null || true
  echo "✓ Installed. Logs:  log stream --predicate 'process == \"sentinel-edge\"'"
fi

echo "✓ sentinel-edge is up. It joined the tailnet as '$HOSTNAME_' and pulls cameras from $CONTROL_URL."
