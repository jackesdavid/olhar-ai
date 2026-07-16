#!/usr/bin/env bash
# Build the Sentinel cloud images locally, push to GHCR, and roll the VPS -- the manual
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
ALL="sentinel-control sentinel-gateway sentinel-flow sentinel-notification sentinel-app sentinel-landing"
SERVICES="${*:-$ALL}"
SHA="$(git rev-parse HEAD)"

command -v docker >/dev/null 2>&1 || { echo "docker not found -- install/start Docker Desktop"; exit 1; }
docker info >/dev/null 2>&1 || { echo "Docker daemon not running -- start Docker Desktop"; exit 1; }

# Per-service Dockerfile + build-arg (mirrors the deploy.yml matrix).
# NOTE: sentinel-postgres is intentionally NOT in ALL — a routine deploy must not restart
# the database. Ship it explicitly: task cloud-deploy -- sentinel-postgres
dockerfile_for() { case "$1" in
  sentinel-app)      echo "deploy/app/Dockerfile" ;;
  sentinel-landing)  echo "deploy/landing/Dockerfile" ;;
  sentinel-postgres) echo "deploy/postgres/Dockerfile" ;;
  *)                 echo "Dockerfile" ;;
esac; }
buildarg_for() { case "$1" in
  sentinel-app)      echo "VITE_COMMIT=${SHA}" ;;
  sentinel-landing)  echo "" ;;
  sentinel-postgres) echo "" ;;
  *)                 echo "PACKAGE=$1" ;;
esac; }

echo ">> Logging in to GHCR as ${GH_USER}"
gh auth token --user "${GH_USER}" 2>/dev/null | docker login ghcr.io -u "${GH_USER}" --password-stdin 2>/dev/null \
  || { gh auth switch -u "${GH_USER}" >/dev/null 2>&1 || true; gh auth token | docker login ghcr.io -u "${GH_USER}" --password-stdin; }

for svc in ${SERVICES}; do
  df="$(dockerfile_for "${svc}")"; ba="$(buildarg_for "${svc}")"
  echo ">> Building ${svc} (${df}${ba:+, ${ba}})"
  docker buildx build --platform linux/amd64 -f "${df}" ${ba:+--build-arg "${ba}"} -t "${REPO}/${svc}:latest" --load .
  echo ">> Pushing ${svc}"
  docker push "${REPO}/${svc}:latest"
done

EDGEVER="$(grep -m1 '^version' sentinel-edge/Cargo.toml | sed -E 's/.*"([^"]+)".*/\1/')"

# The VPS compose is hand-maintained (it holds filled placeholders: __DB_PASS__,
# __EDGE_HOST__…), so we never copy the repo's copy over it. But sentinel-postgres now needs
# two runtime flags the stock pgvector image never had, or `CREATE EXTENSION pg_cron`
# (migration 0046) fails and control won't boot. Patch those in place, idempotently, and
# only when postgres is actually being deployed — a routine deploy must not touch the DB.
PATCH_PG=""
case " ${SERVICES} " in *" sentinel-postgres "*) PATCH_PG=1 ;; esac

echo ">> Rolling the VPS (${VPS_SSH}) -- services: ${SERVICES} (edge ${EDGEVER})"
ssh -i "${VPS_KEY}" -o IdentitiesOnly=yes "${VPS_SSH}" "set -e; cd /ibbf
  sed -i -E 's/(LATEST_EDGE_VERSION: ).*/\\1\"${EDGEVER}\"/' docker-compose.sentinel.yml

  if [ -n '${PATCH_PG}' ]; then
    cp docker-compose.sentinel.yml \"docker-compose.sentinel.yml.bak.\$(date +%s)\"
    sed -i 's|image: pgvector/pgvector:pg17|image: ghcr.io/jackesdavid/sentinel-postgres:latest|' docker-compose.sentinel.yml
    grep -q 'cron.database_name' docker-compose.sentinel.yml || \
      sed -i '\\|image: ghcr.io/jackesdavid/sentinel-postgres:latest|a\\    command: [\"postgres\", \"-c\", \"shared_preload_libraries=pg_cron\", \"-c\", \"cron.database_name=sentinel\"]' docker-compose.sentinel.yml
    echo '>> patched compose for pg_cron'
  fi

  # CLEAN=1: an interrupted prior deploy can leave a half-recreated container squatting the service
  # name (\"Conflict. The container name ... is already in use\"), which then blocks every future
  # \`up -d\`. Force-remove any container matching each service name first so compose recreates a
  # fresh one. Removing a container never touches its named volumes, so this is safe for stateful
  # services (e.g. postgres) too — only the container is replaced, the data volume persists.
  if [ -n '${CLEAN}' ]; then
    for svc in ${SERVICES}; do
      echo \">> clean: dropping any container matching '\$svc'\"
      docker rm -f \$(docker ps -aq --filter name=\"\$svc\") 2>/dev/null || true
    done
  fi

  docker compose -f docker-compose.yml -f docker-compose.sentinel.yml pull ${SERVICES}
  docker compose -f docker-compose.yml -f docker-compose.sentinel.yml up -d ${SERVICES}

  if [ -n '${PATCH_PG}' ]; then
    for i in \$(seq 1 60); do
      docker exec sentinel-postgres pg_isready -U sentinel -d sentinel >/dev/null 2>&1 && break
      sleep 2
    done
    # Abort before anyone deploys control against a scheduler-less database.
    docker exec sentinel-postgres psql -U sentinel -d sentinel -tAc 'SHOW shared_preload_libraries' | grep -q pg_cron \\
      || { echo 'FATAL: pg_cron is not preloaded -- do NOT deploy control'; exit 1; }
    echo '>> pg_cron preloaded OK'
  fi

  docker image prune -f"

echo "OK: deployed ${SERVICES} (commit ${SHA:0:7}, edge ${EDGEVER})"
