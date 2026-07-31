#!/usr/bin/env sh
# sentinel-edge — one-command appliance installer.
#
# A client runs this ONCE on their on-prem box. It:
#   1. installs the GStreamer runtime,
#   2. downloads the sentinel-edge binary,
#   3. writes the config,
#   4. installs a service (systemd / launchd) so it runs 24/7 and on boot,
#   5. starts it.
#
# The web app (Settings → Edge boxes) hands the client a ready-made command with the
# box token + secrets filled in:
#
#   curl -fsSL https://control.olharai.com/install.sh | sudo sh -s -- \
#     --control https://control.olharai.com \
#     --box-token sbx_xxxxxxxx \
#     --hostname tenant-001
#
# The box token binds this appliance to its tenant: control returns only that tenant's cameras.
# No tailnet (card #144) — the edge is outbound-only.
set -eu

# ---- config (flags > env > default) ----------------------------------------
HOSTNAME_="${SENTINEL_HOSTNAME:-sentinel-edge}"
CONTROL_URL="${SENTINEL_CONTROL_URL:-}"
BOX_TOKEN="${SENTINEL_BOX_TOKEN:-}"
PUBLIC_HOST="${SENTINEL_PUBLIC_HOST:-}"
# Where to fetch the sentinel-edge binary. Defaults to the control plane (the repo is
# private, so control serves the binaries at /edge/<asset>); override if self-hosting.
RELEASE_BASE="${SENTINEL_RELEASE_BASE:-}"
PREFIX="${SENTINEL_PREFIX:-/opt/sentinel-edge}"
ENV_DIR="${SENTINEL_ENV_DIR:-/etc/sentinel-edge}"
UPDATE=0   # --update: refresh the payload + restart, keeping env / service / enrollment
FORCE=0    # --force: install the published build even if it is not newer (explicit user request)

while [ $# -gt 0 ]; do
  case "$1" in
    --hostname) HOSTNAME_="$2"; shift 2 ;;
    --control) CONTROL_URL="$2"; shift 2 ;;
    --box-token) BOX_TOKEN="$2"; shift 2 ;;
    --public-host) PUBLIC_HOST="$2"; shift 2 ;;
    --release-base) RELEASE_BASE="$2"; shift 2 ;;
    # Accepted and ignored: GStreamer is the only media path now (card #138). Kept so an older
    # copy-pasted install command does not die on "unknown flag".
    --media-backend) shift 2 ;;
    --update) UPDATE=1; shift ;;
    --force) FORCE=1; shift ;;
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
  Linux)
    # One static musl asset per arch — no glibc floor, runs on any distro (Debian/Ubuntu, Fedora/
    # Bazzite, …). The edge is pure Rust (#79) + rustls, so there's nothing to dynamically link.
    # (This replaced the old apt→`-gnu` / dnf→default split now that CI builds musl in release.yml.)
    EDGE_ASSET="sentinel-edge-linux-$arch" ;;
  Darwin) EDGE_ASSET="sentinel-edge-macos-$arch" ;;
  *) echo "Unsupported OS: $os" >&2; exit 1 ;;
esac
# Reject a platform we do not build for, before anything is downloaded.
case "$os-$arch" in
  Linux-x86_64|Linux-aarch64|Darwin-arm64|Darwin-x86_64) ;;
  *) echo "Unsupported platform: $os-$arch" >&2; exit 1 ;;
esac

# Immutable/ostree host (Bazzite, Silverblue, Kinoite): the base OS is read-only, so package
# installs go through `rpm-ostree` and only take effect on the NEXT boot. We detect it so the
# GStreamer runtime install uses rpm-ostree.
IMMUTABLE=0
REBOOT_REQUIRED=0
if [ "$os" = "Linux" ] && [ -f /run/ostree-booted ]; then IMMUTABLE=1; fi

# Friendly platform label for the logs (matches what the edge reports to the Boxes page).
PLATFORM="$os-$arch"
if [ "$os" = "Linux" ] && [ -r /etc/os-release ]; then
  PLATFORM="$(. /etc/os-release 2>/dev/null; echo "${NAME:-Linux} ($arch)")"
elif [ "$os" = "Darwin" ]; then
  PLATFORM="macOS ($arch)"
fi
echo "▸ Platform: $PLATFORM$( [ "$IMMUTABLE" = 1 ] && echo ' (immutable / rpm-ostree)')"

# ---- what is published FOR THIS PLATFORM ------------------------------------
# ALWAYS pass ?platform=. Without it /edge-latest answers with the LOWEST version across every
# platform — which is how a macOS box on 0.5.66 asked for "latest", was told Linux's 0.5.65, and
# DOWNGRADED itself. Each platform is published independently; only its own entry is meaningful.
case "$os-$arch" in
  Darwin-*)       MANIFEST_KEY="macos" ;;
  Linux-x86_64)   MANIFEST_KEY="linux-x86_64" ;;
  Linux-aarch64)  MANIFEST_KEY="linux-aarch64" ;;
  *)              MANIFEST_KEY="$os-$arch" ;;
esac

# One field out of the /edge-latest JSON.  json_field <json> <name>
json_field() {
  printf '%s' "$1" | sed -n "s/.*\"$2\"[[:space:]]*:[[:space:]]*\"\([^\"]*\)\".*/\1/p"
}

LATEST_JSON="$(curl -fsSL "$CONTROL_URL/edge-latest?platform=$MANIFEST_KEY" 2>/dev/null || true)"
EDGE_VERSION="$(json_field "$LATEST_JSON" version)"
# The published FILENAME, taken from the manifest rather than reconstructed. Naming has changed
# across releases (`Sentinel-Edge-0.5.65.pkg` vs `Sentinel-Edge-0.5.67-gst-x86_64.pkg`), so a name
# built by convention here 404s the moment the convention moves. The manifest is the only authority.
EDGE_FILE="$(json_field "$LATEST_JSON" file)"
if [ -n "$EDGE_VERSION" ]; then
  echo "▸ sentinel-edge version: v$EDGE_VERSION ($MANIFEST_KEY)"
else
  echo "▸ sentinel-edge version: (nothing published for $MANIFEST_KEY at $CONTROL_URL)"
fi

# ---- update mode: keep env / service / enrollment; swap the payload ----------
# Per-platform, because the payload shape differs:
#  • Linux  — a bare gst-linked binary: swap it + restart.
#  • macOS  — a SIGNED .pkg bundle (binary + its OWN bundled GStreamer runtime). A binary-only
#             swap would mismatch the bundle (and the stale raw macOS binary isn't even published),
#             so re-run the NOTARIZED installer: it replaces binary + gst runtime, reruns the
#             postinstall, and keeps enrollment (/etc/sentinel-edge is preserved). This is what
#             makes the in-app "Update" button work on macOS.
if [ "$UPDATE" = "1" ]; then
  [ -x "$PREFIX/sentinel-edge" ] || { echo "Not installed at $PREFIX — run the full installer first." >&2; exit 1; }
  # Resolved once above, FOR THIS PLATFORM, from the manifest. Refuse rather than guess: an update
  # that cannot name what it is installing is how a box ends up on someone else's build.
  [ -n "$EDGE_VERSION" ] && [ -n "$EDGE_FILE" ] || {
    echo "could not resolve a published build for $MANIFEST_KEY from $CONTROL_URL/edge-latest" >&2
    exit 1
  }

  # ---- the safety gate that reaches EVERY box ---------------------------------
  # This script is fetched fresh from control on every update, so the decision made here applies
  # even to boxes whose own binary has broken update logic. That makes this — not the binary — the
  # right place to enforce "only ever move forward".
  #
  # Two failures it prevents:
  #  • DOWNGRADE. A box asking without a platform is told the highest version published anywhere,
  #    which may be another platform's. Without this gate it would install its own platform's older
  #    build and go backwards.
  #  • RESTART LOOP. That same box, already current for its platform, would reinstall the identical
  #    version and restart — then decide again on the next boot, forever.
  INSTALLED=""
  if [ -x "$PREFIX/sentinel-edge" ]; then
    INSTALLED="$("$PREFIX/sentinel-edge" --version 2>/dev/null | tr -d 'a-zA-Z -')"
  fi
  # dotted version → comparable integer (each component assumed < 1000).
  #
  # Uses parameter expansion rather than `IFS=. ; set -- $v`: that idiom relies on word-splitting an
  # unquoted expansion, which zsh does NOT do by default. Under zsh the split silently yields one
  # field, the arithmetic sees "0.5.68", and the function errors out — and an errored comparison here
  # reads as "not newer", i.e. a silently skipped update. Too quiet a failure for this path.
  # Non-numeric components degrade to 0 instead of aborting.
  ver_num() {
    _v="$1"
    _a="${_v%%.*}"; _r="${_v#*.}"
    _b="${_r%%.*}"; _c="${_r#*.}"; _c="${_c%%.*}"
    case "$_a" in ''|*[!0-9]*) _a=0 ;; esac
    case "$_b" in ''|*[!0-9]*) _b=0 ;; esac
    case "$_c" in ''|*[!0-9]*) _c=0 ;; esac
    printf '%d' $(( _a * 1000000 + _b * 1000 + _c ))
  }
  if [ "$FORCE" != "1" ] && [ -n "$INSTALLED" ]; then
    if [ "$(ver_num "$EDGE_VERSION")" -le "$(ver_num "$INSTALLED")" ]; then
      echo "✓ Already on v$INSTALLED; published for $MANIFEST_KEY is v$EDGE_VERSION — nothing to do."
      exit 0
    fi
  fi
  if [ "$os" = "Linux" ]; then
    echo "▸ Updating sentinel-edge ($MANIFEST_KEY) → v${EDGE_VERSION} from ${RELEASE_BASE}…"
    curl -fSL "$RELEASE_BASE/$EDGE_FILE" -o "$PREFIX/sentinel-edge.new"
    chmod +x "$PREFIX/sentinel-edge.new"
    mv "$PREFIX/sentinel-edge.new" "$PREFIX/sentinel-edge"
    systemctl restart sentinel-edge
    echo "✓ Updated to v${EDGE_VERSION}. Status: systemctl status sentinel-edge"
  else
    pkg="$(mktemp -d)/$EDGE_FILE"
    echo "▸ Updating sentinel-edge (macOS) → v${EDGE_VERSION} via signed .pkg reinstall…"
    curl -fSL "$RELEASE_BASE/$EDGE_FILE" -o "$pkg"
    installer -pkg "$pkg" -target /   # replaces binary + gst runtime, reruns postinstall (keeps identity)
    rm -f "$pkg"
    echo "✓ Updated to v${EDGE_VERSION}."
  fi
  exit 0
fi

echo "▸ Installing sentinel-edge ($os-$arch) → $PREFIX"
mkdir -p "$PREFIX" "$PREFIX/recordings" "$ENV_DIR"

# ---- 1. No tailnet (card #144) ----------------------------------------------
# The edge is outbound-only: it reaches control over HTTPS, holds one outbound SSE command stream,
# and pushes live video out to the gateway. Nothing needs to reach INTO the box, so there is no
# Tailscale to install or join — one fewer dependency and zero inbound surface.

# ---- runtime dep: GStreamer -------------------------------------------------
# retired. Install the GStreamer runtime + plugins so the linked pipeline finds its elements (decode
# incl. HW where available, mux, appsink). The edge is outbound-only (card #144: SSE commands + live
# plugins layer via rpm-ostree; the plugin list was proven on the Jetson test box (`-libav` is
# load-bearing — some boxes ship no software H264/H265 decoder without it).
if ! command -v gst-inspect-1.0 >/dev/null 2>&1; then
  echo "▸ Installing GStreamer runtime…"
  if [ "$os" = "Linux" ]; then
    if [ "$IMMUTABLE" = 1 ]; then
      rpm-ostree install --idempotent --allow-inactive \
        gstreamer1 gstreamer1-plugins-base gstreamer1-plugins-good \
        gstreamer1-plugins-bad-free gstreamer1-plugins-ugly-free gstreamer1-libav \
        || echo "  ⚠ rpm-ostree install failed — layer GStreamer manually" >&2
      command -v gst-launch-1.0 >/dev/null 2>&1 || REBOOT_REQUIRED=1
    elif command -v apt-get >/dev/null 2>&1; then
      # `-libav` is load-bearing: some boxes (e.g. Jetson/L4T) ship base/good/bad/ugly WITHOUT a
      # software H264/H265 decoder, so `decodebin` fails with "missing plugin" until this is
      # installed too (found running the Jetson test box).
      apt-get update -qq && apt-get install -y \
        gstreamer1.0-tools gstreamer1.0-plugins-base gstreamer1.0-plugins-good \
        gstreamer1.0-plugins-bad gstreamer1.0-plugins-ugly gstreamer1.0-libav
    elif command -v dnf >/dev/null 2>&1; then
      dnf install -y gstreamer1 gstreamer1-plugins-base gstreamer1-plugins-good \
        gstreamer1-plugins-bad-free gstreamer1-plugins-ugly-free gstreamer1-libav
    elif command -v apk >/dev/null 2>&1; then
      apk add --no-cache gstreamer gst-plugins-base gst-plugins-good gst-plugins-bad \
        gst-plugins-ugly gst-libav
    else
      echo "  No supported package manager found — install GStreamer manually, then re-run." >&2
    fi
  else
    # macOS: Homebrew, same non-root pattern as the Linux package install above.
    if [ -n "${SUDO_USER:-}" ] && command -v sudo >/dev/null 2>&1 && sudo -u "$SUDO_USER" command -v brew >/dev/null 2>&1; then
      sudo -u "$SUDO_USER" brew install gstreamer gst-plugins-base gst-plugins-good gst-plugins-bad gst-plugins-ugly gst-libav || true
    elif command -v brew >/dev/null 2>&1; then
      brew install gstreamer gst-plugins-base gst-plugins-good gst-plugins-bad gst-plugins-ugly gst-libav || true
    else
      echo "  Install GStreamer first:  brew install gstreamer gst-plugins-{base,good,bad,ugly} gst-libav  — then re-run." >&2
    fi
  fi
fi
if command -v gst-inspect-1.0 >/dev/null 2>&1; then
  echo "✓ GStreamer present ($(command -v gst-inspect-1.0))"
else
  echo "⚠ GStreamer NOT found — the edge has no media backend until it is installed." >&2
fi

# ---- 2. binaries ------------------------------------------------------------
# Prefer the manifest's filename; fall back to the conventional name only when nothing is published
# for this platform (a box installing against a control that predates the manifest).
DOWNLOAD_AS="${EDGE_FILE:-$EDGE_ASSET}"
echo "▸ Downloading sentinel-edge ($DOWNLOAD_AS)…"
curl -fSL "$RELEASE_BASE/$DOWNLOAD_AS" -o "$PREFIX/sentinel-edge"
chmod +x "$PREFIX/sentinel-edge"
# Confirm the installed build's version. The binary has no native dependencies (#79), so this
# always runs — on any host, immutable or not, before any reboot.
if ver="$("$PREFIX/sentinel-edge" --version 2>/dev/null)"; then
  echo "✓ Installed $ver"
fi

# to download; the GStreamer runtime installed above provides the elements the edge uses in-process.

# ---- 3. config --------------------------------------------------------------
# Bake a PATH so the service can find its runtime deps
# even under a minimal systemd/launchd PATH (Homebrew dirs are NOT on a daemon's default).
EDGE_PATH="/usr/local/bin:/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin"

echo "▸ Writing $ENV_DIR/edge.env"
cat > "$ENV_DIR/edge.env" <<EOF
# Generated by install.sh — sentinel-edge appliance config.
EDGE_BIND=127.0.0.1:8788
CONTROL_URL=$CONTROL_URL
BOX_TOKEN=$BOX_TOKEN
EDGE_CACHE=$PREFIX/cameras-cache.json
PATH=$EDGE_PATH
EOF
chmod 600 "$ENV_DIR/edge.env"   # contains the box token

# edge defaults to GStreamer and reaches the cloud outbound-only (card #144).

# ---- 4. service ------------------------------------------------------------
if [ "$os" = "Linux" ]; then
  echo "▸ Installing systemd service…"
  cat > /etc/systemd/system/sentinel-edge.service <<EOF
[Unit]
Description=Sentinel edge gateway (cameras · in-process GStreamer · outbound-only)
After=network-online.target
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
  # Always start it. This used to be gated on REBOOT_REQUIRED, because the edge linked OpenCV and
  # would crash-loop until the rpm-ostree-layered libs became active on the next boot. It links
  # nothing now (#79), so it runs immediately on an immutable host too.
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
if [ "$REBOOT_REQUIRED" = 1 ]; then
  echo ""
  echo "ℹ  GStreamer was layered via rpm-ostree and activates on next boot. The edge is already"
  echo "   running (it links nothing); until you reboot, only snapshots + GetCurrentFrame will"
  echo "   fail — live video, motion, tracking and recording all work.  sudo systemctl reboot"
fi
