# OlharAI cloud stack — dedicated-server standup

Greenfield deploy of the whole stack on the new dedicated server, under `olharai.com`. Card **#142**;
plan: [`docs/plans/olharai-infra-migration.md`](../../docs/plans/olharai-infra-migration.md).

This dir is self-contained: `docker-compose.olharai.yml` bundles **Traefik + Let's Encrypt** (the old
ibbf VPS shared an external Traefik; this box is dedicated). The `sentinel-*` app services run **2
replicas** each; datastores (postgres/redis/minio) stay single.

## Placeholders to fill (in the compose, on the server)

`docker-compose.olharai.yml` has literal placeholders — replace before first `up`:

| Placeholder | What |
|---|---|
| `__DB_PASS__` | Postgres password (matches `POSTGRES_PASSWORD`) — pick a strong one |
| `__ACME_EMAIL__` | email for Let's Encrypt registration |
| `__EDGE_HOST__` | the fallback edge's MagicDNS host (tailnet) — same idea as the old stack |
| `__EDGE_TS_IP__` | that edge's Tailscale IP (for the `extra_hosts` entry) |

And `./sentinel.env` — copy from `sentinel.env.example`, fill, **do not commit**.

## Standup order (postgres FIRST — migration 0046 needs pg_cron)

```sh
cd /olharai            # or wherever this dir lives on the server
cp sentinel.env.example sentinel.env && $EDITOR sentinel.env    # fill secrets
$EDITOR docker-compose.olharai.yml                              # fill __PLACEHOLDERS__
mkdir -p edge-dist                                             # edge binaries land here (release CI)

# 1. Postgres first, wait healthy.
docker compose -f docker-compose.olharai.yml up -d sentinel-postgres
# 2. Then control (runs migrations + seeds master, permissions, L1–L6 templates on the empty DB).
docker compose -f docker-compose.olharai.yml up -d sentinel-control
# 3. Then the rest (Traefik, app, gateway, landing, flow, notification, ai, minio + init).
docker compose -f docker-compose.olharai.yml up -d
```

## ⚠️ DNS is the cutover — operator does it

**Before** DNS points at this box, verify locally by faking the hostnames in `/etc/hosts` (or curl
with `--resolve`). Only when the stack answers correctly does the operator point Cloudflare:

- `olharai.com`, `app.olharai.com`, `control.olharai.com`, `gateway.olharai.com` → the new server IP.
- Traefik provisions Let's Encrypt certs automatically on first request per `Host()` rule.

## Replicas

2 per `sentinel-*` service is baked in (`deploy.replicas: 2`). Change one at runtime without a
redeploy:

```sh
docker compose -f docker-compose.olharai.yml up -d --scale sentinel-control=3
```

Datastores are single by design — don't scale postgres/redis/minio (see the plan's Phase 1b).

## What still points at the old world (handled elsewhere)

- **CI / local deploy target** — `secrets.VPS_HOST` + `deploy/push-local.sh` + `Taskfile.yml` still
  aim at the old host; retargeting them is Phase 2 of the plan.
- **External integrations** (Google OAuth redirect URIs, Stripe dashboard URLs/webhooks, Telegram,
  Tuya) — batched at the end (plan Phase 4), after this stack is verified.
- **Edge boxes** — reinstalled fresh against `control.olharai.com` (plan Phase 3).
- **Brand strings / landing copy / the mark** — separate effort (`docs/plans/olharai-rebrand.md`).
