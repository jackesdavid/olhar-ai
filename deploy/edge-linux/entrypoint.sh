#!/usr/bin/env bash
# Container entrypoint for the sentinel-edge appliance on an immutable host (Bazzite /
# Fedora Silverblue / any rpm-ostree box), where layering packages means a reboot.
#
# The edge links nothing itself (card #79 — motion is pure Rust), so this container is a
# convenience, not a requirement. It is outbound-only (card #144): it dials the cloud and needs
# no inbound reachability, no VPN, no tun device and no NET_ADMIN.
#
#   podman run -d --name sentinel-edge --restart=always \
#     --network=host \
#     -v sentinel-edge-data:/opt/sentinel-edge \
#     --env-file /etc/sentinel-edge/edge.env \
#     registry.gitlab.com/olharai-grupo/olharai/sentinel-edge:latest
#
# Host networking stays because the edge discovers cameras on the LAN.
# The env file is the SAME one install.sh writes (CONTROL_URL, BOX_TOKEN, …).
set -eu

exec /usr/local/bin/sentinel-edge
