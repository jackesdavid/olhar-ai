#!/usr/bin/env bash
# Build the Sentinel cloud images locally, push to GHCR, and roll the VPS — the manual
# equivalent of .github/workflows/deploy.yml, for when GitHub Actions isn't available
# (e.g. billing). Builds linux/amd64 (the VPS arch) regardless of your host.
#
#   ./deploy/push-local.sh                                   # all services
#   ./deploy/push-local.sh sentinel-control sentinel-flow    # only these
#
# Requires: Docker Desktop running; `gh` logged in as the package owner (write:packages);
# SSH access to the VPS. Override VPS_SSH / VPS_KEY if your host/key differ.
set -euo pipefail
cd "$(dirname "$0")/.."  # repo root

REPO="ghcr.io/jackesdavid"
GH_USER="jackesdavid"
VPS_SSH="${VPS_SSH:-root@85.239.248.86}"
VPS_KEY="${VPS_KEY:-$HOME/.ssh/id_contab_ed25519}"
ALL="sentinel-control sentinel-gateway sentinel-flow sentinel-app sentinel-landing"
SERVICES="${*:-$ALL}"
SHA="$(git rev-parse HEAD)"

command -v docker >/dev/null 2>&1 || { echo "✗ docker not found — install/start Docker Desktop"; exit 1; }
docker info >/dev/null 2>&1 || { echo "✗ Docker daemon not running — start Docker Desktop"; exit 1; }

# Per-service Dockerfile + build-arg (mirrors the deploy.yml matrix).
dockerfile_for() { case "$1" in
  sentinel-app)     echo "deploy/app/Dockerfile" ;;
  sentinel-landing) echo "deploy/landing/Dockerfile" ;;
  *)                echo "Dockerfile" ;;
esac; }
buildarg_for() { case "$1" in
  sentinel-app)     echo "VITE_COMMIT=$SHA" ;;
  sentinel-landing) echo "" ;;
  *)                echo "PACKAGE=$1" ;;
esac; }

echo "▸ Logging in to GHCR as $GH_USER…"
gh auth token --user "$GH_USER" 2>/dev/null | docker login ghcr.io -u "$GH_USER" --password-stdin 2>/dev/null \
  || { gh auth switch -u "$GH_USER" >/dev/null 2>&1 || true; gh auth token | docker login ghcr.io -u "$GH_USER" --password-stdin; }

for svc in $SERVICES; do
  df="$(dockerfile_for "$svc")"; ba="$(buildarg_for "$svc")"
  echo "▸ Building $svc ($df${ba:+, $ba})…"
  docker buildx build --platform linux/amd64 -f "$df" ${ba:+--build-arg "$ba"} -t "$REPO/$svc:latest" --load .
  echo "▸ Pushing $svc…"
  docker push "$REPO/$svc:latest"
done

EDGEVER="$(grep -m1 '^version' sentinel-edge/Cargo.toml | sed -E 's/.*"([^"]+)".*/\1/')"
echo "▸ Rolling the VPS ($VPS_SSH) — services: $SERVICES (edge $EDGEVER)…"
ssh -i "$VPS_KEY" -o IdentitiesOnly=yes "$VPS_SSH" "set -e; cd /ibbf
  sed -i -E 's/(LATEST_EDGE_VERSION: ).*/\\1\"$EDGEVER\"/' docker-compose.sentinel.yml
  docker compose -f docker-compose.yml -f docker-compose.sentinel.yml pull $SERVICES
  docker compose -f docker-compose.yml -f docker-compose.sentinel.yml up -d $SERVICES
  docker image prune -f"

echo "✓ Deployed: $SERVICES  (commit ${SHA:0:7}, edge $EDGEVER)"
