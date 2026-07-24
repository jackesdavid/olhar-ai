# OlharAI Infra Migration — new server + full domain cutover to olharai.com

**Board card:** #142   **Related:** `docs/plans/olharai-rebrand.md` (brand strings + code domain refs
— that plan does the *code*; this one does the *infrastructure*).
**Goal:** Move the whole stack off the current **shared** Contabo VPS onto a **new dedicated server**,
cut the public domain from `thesentinels.online` to `olharai.com`, and **fully retire** the old
domain — with **no data loss** and **no orphaned edge boxes**.

> **Secrets:** the new server's IP, user, and password are held out-of-band (shared in chat, not in
> this repo). **Rotate that password and switch to SSH-key auth** as the first step — a password
> that travelled over chat should not be the production credential.

## Why this is more than a DNS change

The current VPS (`85.239.248.86`) is **shared with the church app** (`ibbf.life`); the Sentinel stack
is an *additive overlay* (`deploy/vps/docker-compose.sentinel.yml`, "does not touch the church app").
Moving to a dedicated box is a clean separation — but it means standing up Traefik + Let's Encrypt +
the full stack fresh, and **migrating two stateful volumes**, not just repointing DNS.

**Stateful data that MUST migrate (loss = irreversible):**
- **Postgres** `sentinel_pgdata` — every tenant, user, workflow, event, face, push device. The core.
- **MinIO** `sentinel_minio` — recordings/notification media live in *local* MinIO on the server
  (`S3_ENDPOINT: http://sentinel-minio:9000`), not only external R2. Skip this and clips 404.

**External state (no migration, just re-point):**
- Cloudflare DNS (no IaC — managed in the dashboard).
- Google OAuth redirect URIs, Stripe success/cancel URLs — registered externally, updated per
  `olharai-rebrand.md` Phase 1.

## The gating constraint — deployed edge boxes

Every edge appliance in the field stores `CONTROL_URL=https://control.thesentinels.online` in its
env file (`sentinel-edge/src/envfile.rs`), **set once at install**. The self-update
(`main.rs::maybe_self_update`) updates only the *binary* and re-passes the **same** control URL —
there is **no remote mechanism to change it**. Therefore:

**`control.thesentinels.online` cannot be torn down until every edge is re-pointed.** "Fully retire
the old domain" splits into two different deadlines:
- **User-facing surfaces** (`app.`, `landing`, `gateway.` public) → can cut to `olharai.com` fast.
- **`control.thesentinels.online`** → must **keep resolving to the new server** as a machine
  endpoint until the last edge is migrated, then retire. See Phase 4.

## Phase 0 — Access + prep (new server, nothing user-facing yet)
- Rotate the password; add an SSH **key** for the deploy user; confirm `sudo`, Docker + Compose
  plugin, and that ports 80/443 are open and not already bound.
- Confirm the box's resources vs the current footprint (postgres+pgvector, redis, minio, control,
  flow, gateway, app, landing, notification, ai-sidecar — ~10 containers).
- **Non-root note:** current deploy assumes `root@`. Either give the deploy user Docker-group
  access + a writable deploy dir (mirror `/ibbf/`), or provision as root. Decide the deploy dir
  path (was `/ibbf/`; pick an OlharAI-neutral one, e.g. `/olharai/`).

## Phase 1 — Stand up the new stack (parallel; old stays live)
- Install Traefik + Let's Encrypt on the new box (or fold Traefik into the compose — it was external
  before, shared with the church app; now it can be part of this stack).
- Put the secrets file on the new box (was `/ibbf/sentinel.env`): `JWT_SECRET`, `GOOGLE_*`,
  `MASTER_*`, `ANTHROPIC_API_KEY`, `S3_KEY/S3_SECRET`, DB pass. **Reuse `JWT_SECRET` verbatim** or
  every existing session/token is invalidated on cutover — decide (rotating it forces all users +
  **all box tokens** to re-auth, which compounds the edge problem; lean: reuse).
- Deploy the stack from GHCR (same images CI already builds) with the new compose.
- **Migrate data** into the new volumes, old stack still serving:
  - Postgres: `pg_dump` on old → restore on new (or `docker run` a base-image dump/restore; a
    volume rsync also works but dump/restore is safer across engine restarts). Validate row counts.
  - MinIO: `mc mirror` old bucket → new, or rsync the `sentinel_minio` volume.
- Smoke-test the new stack via a temp hostname / `/etc/hosts` override before any DNS points at it.

## Phase 2 — Domain cutover to olharai.com (new server)
- Cloudflare: add `olharai.com` (+ `.com.br` redirect) per `olharai-rebrand.md` Phase 1; A/CNAME for
  root(landing) / `app.` / `control.` / `gateway.` → **new server IP**.
- New compose Traefik `Host()` rules on `olharai.com` (dedicated box, so no dual-tenant concern; the
  church-app coexistence constraint is gone).
- Land the code domain changes from `olharai-rebrand.md` Phase 1 (`config.ts`, `stripe.rs`,
  `OAUTH_REDIRECT_BASE`/`OAUTH_SUCCESS_REDIRECT`, `sentinel-edge/install.sh`, app OG URLs).
- Update Google OAuth redirect URIs + Stripe URLs (external dashboards).
- **DNS flip = the cutover.** Lower TTLs beforehand; keep the old stack warm until the new one is
  taking real traffic and verified.

## Phase 3 — Retarget CI + local deploy
- GitHub Actions: update `secrets.VPS_HOST` (+ deploy SSH key secret) to the new server, across
  `deploy.yml`, `release.yml`, `ai-sidecar.yml`.
- Un-hardcode `root@85.239.248.86` + `id_contab_ed25519` in `deploy/push-local.sh` and `Taskfile.yml`
  (the `edge-publish`/`edge-dist` scp targets) → new host/user/key/path. **Note the release path
  `/ibbf/edge-dist`** (served as `control…/edge/*`) moves too, or edge downloads 404.

## Phase 4 — Edge migration, then retire control.thesentinels.online (the careful part)
- **Keep `control.thesentinels.online` DNS pointed at the NEW server** (add it to the new Traefik
  `Host()` rule alongside `control.olharai.com`) so field edges keep working through the move — the
  `gateway.ibbf.life` legacy-alias pattern already in the compose is the exact precedent.
- Re-point each edge to `control.olharai.com`. Options (pick per fleet size):
  1. **Re-install** with `--control https://control.olharai.com` (the install.sh `--update` path,
     but `--update` re-passes the old URL, so it must be a fresh `--control`, not a bare update).
  2. **Edit the env file** on each box (`CONTROL_URL=`) + restart — needs box access.
  3. **Build a real remote re-point** — a control-issued command the edge honours on its config
     long-poll to rewrite its own `CONTROL_URL`. This is the only *scalable* answer and is likely
     its **own card** if the fleet is more than a handful. Flagged, not assumed.
- Only when **zero** edges resolve the old control host (verify via access logs / edge health
  reports) → drop `control.thesentinels.online`.

## Phase 5 — Full retirement of thesentinels.online + old overlay teardown
- Once user-facing traffic is on `olharai.com` and all edges are migrated: replace the old domain's
  routers with a 301 → `olharai.com` for a grace window, then remove DNS/records entirely.
- **Decommission the old overlay, NOT the old server** — the box still runs the church app
  (`ibbf.life`). Remove only `sentinel-*` containers, images, and the `sentinel_pgdata` /
  `sentinel_minio` volumes **after** confirming the new server holds the data and has a backup.

## Open decisions
1. **Reuse `JWT_SECRET` or rotate?** Reuse = seamless sessions + box tokens survive; rotate = every
   user and every edge re-auths (worsens the edge problem). Lean reuse.
2. **Traefik: external (as today) or folded into the stack?** Dedicated box makes folding it in
   cleaner and self-contained. Lean fold-in.
3. **Deploy dir + user**: keep `/ibbf/`-style root layout, or an OlharAI-neutral `/olharai/` with a
   non-root deploy user? Ties to the non-root access question in Phase 0.
4. **Edge re-point mechanism** — manual re-install vs. a built remote re-point command. Depends on
   how many boxes are actually deployed (operator knows the count).
5. **MinIO vs R2 going forward** — migration is a chance to move clips to Cloudflare R2 (external,
   no server-side volume to migrate next time) instead of local MinIO. Separate call; out of scope
   here unless decided now.

## Out of scope (v1)
- Brand strings, mark, landing copy — that is `olharai-rebrand.md`, run alongside.
- Any repo/service/crate rename (stays `sentinel-*`, standing instruction).
- Moving clip storage to R2 (open decision #5 — separate if pursued).

## Verification
- New server serves app/control/gateway/landing on `olharai.com` with valid TLS.
- Row counts + a spot-check of clips match between old and new before old is touched.
- A **real edge box** re-pointed to `control.olharai.com` reports health, streams live, uploads a
  clip end-to-end.
- Old domain 301s to `olharai.com`; `control.thesentinels.online` retired only after edge logs show
  zero hits.
- Church app on the old server still up throughout (never touched).
