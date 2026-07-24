# Filma AI Infra Migration — greenfield new server + full domain cutover to filma.ai

**Board card:** #142   **Related:** `docs/plans/filma-rebrand.md` (brand strings + code domain refs
— that plan does the *code*; this one does the *infrastructure*).
**Goal:** Stand up the **entire stack fresh** on a **new dedicated server** under **filma.ai**,
**greenfield** (empty Postgres + MinIO — no data carried over), reinstall every edge box pointing at
the new domain, batch-update the external integrations at the end, then **fully retire**
`thesentinels.online` and tear down the old overlay.

> **Secrets:** the new server's IP, user, and password are held out-of-band (shared in chat, not in
> this repo). The operator will **rotate that password after the migration** (their call). Still
> worth adding an SSH key early so the migration itself isn't driven over password auth.

## Strategy (decided by the operator)

**Greenfield, clean break.** New empty Postgres/MinIO. The operator **reinstalls every edge box** at
the clients from scratch, and updates the external integrations (Stripe, OAuth, …) **at the very end,
in one batch, after the new server is verified up**. This collapses the two hardest risks of a normal
migration:

- **No data migration** — no `pg_dump`, no MinIO mirror. Accepted loss: existing tenants, users,
  workflows, faces, recordings, events, history. The operator recreates what's needed.
- **No edge-orphan constraint** — because every edge is reinstalled fresh against the new control
  host, `control.thesentinels.online` does **not** need to outlive the cutover. It can die entirely.
- **Fresh `JWT_SECRET`** — greenfield means no sessions or box tokens to preserve, so generate a new
  one; nothing to reconcile.

## Grounding facts (still true)

- Current VPS (`85.239.248.86`) is **shared with the church app** (`ibbf.life`); the Sentinel stack
  is an additive overlay (`deploy/vps/docker-compose.sentinel.yml`). Teardown removes the
  `sentinel-*` overlay only — **never touch the church app**.
- Stack: postgres (pgvector + pg_cron), redis, control, flow, gateway, app, landing, notification,
  minio (+init), ai-sidecar — ~11 containers, all from GHCR images CI already builds.
- Traefik + Let's Encrypt front the stack (was external, shared with the church app).
- A fresh DB **auto-seeds**: `seed.rs` upserts the RBAC permission catalog and the master user
  (`MASTER_EMAIL`/`MASTER_PASSWORD`); `seed_templates.rs` seeds the global L1–L6 workflow templates
  owned by the platform tenant. So a greenfield boot already has master login + the template
  curriculum — tenants are then (re)created by signup/master.
- Deploy target is hardcoded in places: CI uses `secrets.VPS_HOST` (easy), but
  `deploy/push-local.sh` and `Taskfile.yml` pin `root@85.239.248.86` + `id_contab_ed25519`, and the
  new server is a **non-root** user.

## Phase 0 — New server prep
- Rotate the password; add an SSH **key** for the deploy user; confirm `sudo`, Docker + Compose
  plugin, ports 80/443 free.
- Pick the deploy dir (was `/ibbf/`; use a brand-neutral `/filma/`) and decide root vs non-root
  deploy user (Docker-group + writable deploy dir if non-root).
- Put a fresh secrets env on the box: **new** `JWT_SECRET`, `GOOGLE_CLIENT_ID/SECRET`,
  `MASTER_EMAIL/PASSWORD`, `ANTHROPIC_API_KEY`, `S3_KEY/S3_SECRET`, DB pass.

## Phase 1 — Stand up the fresh stack (new domain from the first boot)
- Traefik + Let's Encrypt on the new box (fold into the compose now that it's a dedicated host).
- Deploy `sentinel-postgres` **first** (migration 0046 needs pg_cron), then control (runs
  migrations + seeds master, permissions, templates on an empty DB), then the rest.
- New compose Traefik `Host()` rules on `filma.ai` (`app.`, `control.`, `gateway.`, root=landing).
  No dual-domain, no legacy alias — greenfield.
- Bring the code domain changes from `filma-rebrand.md` Phase 1 into the images/config:
  `sentinel-app/src/lib/config.ts`, `crates/app-core/src/billing/payments/stripe.rs`,
  `OAUTH_REDIRECT_BASE`/`OAUTH_SUCCESS_REDIRECT` in the compose, `sentinel-edge/install.sh` default
  `--control`.
- Verify on a temp hostname / `/etc/hosts` before DNS, then point `filma.ai` DNS (Cloudflare) at
  the new server. Master login works; a fresh tenant shows the template curriculum.

## Phase 1b — Two replicas per sentinel-* service (baked into the new compose)

The operator wants **≥2 instances of each `sentinel-*` application service** from day one. The
**datastores stay single** (`sentinel-postgres` explicitly "normal for now", `sentinel-redis`,
`sentinel-minio`) — you replicate the *stateless app tier* that talks to a shared data tier, not the
data itself. This is the correct pattern and the codebase already supports it.

**Already proven in-repo:** `sentinel-flow` runs N replicas today (`docker compose ... up -d --scale
sentinel-flow=N` / `task flow-scale N`): no `container_name`, Docker-assigned hostname → `pod_name`,
Docker DNS round-robins. The `work_jobs` queue claims with `FOR UPDATE SKIP LOCKED` (atomic), so
concurrent workers are safe, and Redis pub/sub is the cross-pod event fabric. So the horizontal-scale
foundations exist; this phase applies flow's pattern to the rest.

**Services to run at 2 replicas** (all stateless — verified: JWT auth, no in-memory session, shared
Postgres/Redis/MinIO):
- `sentinel-control`, `sentinel-flow` (already), `sentinel-gateway`, `sentinel-notification`,
  `sentinel-app`, `sentinel-landing`.

**Compose changes required to make them scalable** (each currently has `container_name`, which
**blocks** replicas):
- **Remove `container_name`** from the six services (a name is unique; two replicas can't share it).
  Flow already omits it — mirror that.
- **No host-published ports** on the scaled services — Traefik reaches container ports directly via
  the Docker provider and **load-balances across all replicas automatically** off the existing
  `Host()` router labels. (Internal-only `sentinel-flow` uses Docker DNS round-robin instead.)
- Set `deploy.replicas: 2` per service (the compose already uses `deploy.replicas` for flow;
  `--scale` / `task *-scale N` still overrides at runtime).
- Anything that referenced a service by its old fixed `container_name` must use the **service name**
  (Docker DNS) so it round-robins — e.g. control's `FLOW_URL` already points at `sentinel-flow`.

**Datastores stay single** — `sentinel-postgres` (operator: leave normal for now),
`sentinel-redis` (a 2nd naive instance splits pub/sub brain), `sentinel-minio` (2 on one volume
corrupts). `sentinel-ai`/`sentinel-ai-dlib` are heavy compute backends — leave at 1 unless the box
has headroom (stateless `runsync`, so scalable later if wanted).

**Honest scope of "2 pods":** this makes the **app tier** HA — zero-downtime rolling restarts on
deploy, load spread, one pod crashing doesn't drop the service. It does **not** make the **data tier**
HA: Postgres/Redis/MinIO are still single points of failure. Full HA (Postgres replication, Redis
Sentinel, distributed MinIO or R2) is a later, larger step — explicitly deferred by the
"Postgres normal for now" decision.

## Phase 2 — Retarget CI + local deploy to the new server
- GitHub Actions: update `secrets.VPS_HOST` + the deploy SSH key secret across `deploy.yml`,
  `release.yml`, `ai-sidecar.yml`.
- Un-hardcode `root@85.239.248.86` / `id_contab_ed25519` in `deploy/push-local.sh` and the
  `edge-publish`/`edge-dist` scp targets in `Taskfile.yml` → new host/user/key/path. The edge
  release path `/ibbf/edge-dist` (served at `control…/edge/*`) moves too, or edge downloads 404.

## Phase 3 — Reinstall the edges (operator, at the clients)
- Fresh install on each box with `--control https://control.filma.ai` (new pairing code →
  new box token, new Tailscale key). No re-point mechanism needed — it's a clean install.
- After this phase, nothing in the field references the old control host.

## Phase 4 — Batch the external integrations (one pass, at the end)
Update every external service that has the old domain in its config, **after** the new stack is
verified:
- **Google OAuth**: redirect URIs → `https://control.filma.ai/...` (Google Cloud console).
- **Stripe**: success/cancel URLs (code default already changed in Phase 1); update any
  dashboard-side URLs / webhook endpoints pointing at the old domain.
- **Telegram @BotFather**: if the bot's webhook or display identity references the old domain.
- **Tuya / any other provider callback** that carries our domain.
- **Cloudflare**: `.com.br` bulk redirect → `filma.ai` (per filma-rebrand.md).
- Audit: `grep -rin thesentinels` for any remaining live config reference.

## Phase 5 — Retire thesentinels.online + tear down the old overlay
- With traffic on `filma.ai`, edges reinstalled, and integrations repointed: 301 the old domain
  → `filma.ai` for a short grace window, then remove its DNS/records entirely (including
  `control.thesentinels.online` — safe now, no edge depends on it).
- **Decommission the old overlay, NOT the old server**: remove only `sentinel-*` containers, images,
  and the `sentinel_pgdata` / `sentinel_minio` volumes. The church app (`ibbf.life`) keeps running
  on that box, untouched.

## Open decisions
1. **Traefik** — fold into the stack (leaning yes, dedicated box) vs. stand up separately.
2. **Deploy dir + user** — `/filma/` + non-root deploy user vs. mirror the old root `/ibbf/` layout.
3. **MinIO vs R2 going forward** — greenfield is the moment to switch clip storage to external
   Cloudflare R2 (no server-side volume to ever migrate again) instead of local MinIO. Separate
   call; decide now or keep MinIO.
4. **Recreate tenants how** — master recreates them, or clients re-signup. Greenfield seeds master +
   templates automatically; tenant recreation is a manual/operational step.
5. **Replica count** — 2 per sentinel-* service is the floor the operator set; confirm the box has
   the RAM/CPU for 2× six services + the datastores + the AI sidecar before committing, and whether
   the AI sidecar also wants ≥2 (heavy; leave at 1 unless there's headroom).

## Out of scope (v1)
- Brand strings, mark, landing copy — that is `filma-rebrand.md`, run alongside.
- Any repo/service/crate rename (stays `sentinel-*`, standing instruction).
- Carrying over ANY data (greenfield — explicit).

## Verification
- New server serves app/control/gateway/landing on `filma.ai` with valid TLS; master logs in; a
  fresh tenant shows the L1–L6 templates.
- A **real edge box**, freshly installed against `control.filma.ai`, enrolls, reports health,
  streams live, and uploads a clip end-to-end.
- `grep -rin thesentinels` finds no remaining **live** config/integration reference.
- Old domain 301s then is removed; the church app on the old server is up throughout (never touched).
