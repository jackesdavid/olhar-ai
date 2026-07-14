#!/usr/bin/env bash
# Container entrypoint for the sentinel-edge appliance on an immutable host (Bazzite /
# Fedora Silverblue / any rpm-ostree box), where layering packages means a reboot.
#
# Everything the edge needs is baked into the image (ffmpeg + go2rtc + tailscale). The edge binary
# itself links nothing (card #79 — motion is pure Rust), so this container is now a convenience,
# not a requirement. We run tailscaled INSIDE the container, so it needs host networking + the tun
# device + NET_ADMIN:
#
#   podman run -d --name sentinel-edge --restart=always \
#     --network=host --cap-add=NET_ADMIN --cap-add=NET_RAW --device=/dev/net/tun \
#     -v sentinel-edge-tailscale:/var/lib/tailscale \
#     -v sentinel-edge-data:/opt/sentinel-edge \
#     --env-file /etc/sentinel-edge/edge.env \
#     ghcr.io/jackesdavid/sentinel-edge:latest
#
# The env file is the SAME one install.sh writes (CONTROL_URL, BOX_TOKEN, TS_AUTHKEY, …).
set -eu

# tailscaled: the edge shells out to the `tailscale` CLI (up/serve/status); it expects the
# daemon already running. Start it in the background with a persisted state dir (the volume),
# using userspace-networking only as a fallback when /dev/net/tun isn't available.
if command -v tailscaled >/dev/null 2>&1; then
  TS_STATE_DIR="${TS_STATE_DIR:-/var/lib/tailscale}"
  mkdir -p "$TS_STATE_DIR"
  if [ -c /dev/net/tun ]; then
    tailscaled --state="$TS_STATE_DIR/tailscaled.state" --socket=/var/run/tailscale/tailscaled.sock &
  else
    echo "⚠ /dev/net/tun missing — tailscaled in userspace-networking mode (pass --device=/dev/net/tun for kernel networking)" >&2
    tailscaled --state="$TS_STATE_DIR/tailscaled.state" --socket=/var/run/tailscale/tailscaled.sock --tun=userspace-networking &
  fi
  # Give the daemon a moment so the edge's first `tailscale up` can reach it.
  for _ in $(seq 1 10); do tailscale status >/dev/null 2>&1 && break; sleep 1; done
fi

# The edge owns go2rtc (EDGE_OWN_GO2RTC=1) and re-asserts tailscale on boot via ensure_up().
exec /usr/local/bin/sentinel-edge
