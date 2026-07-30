# Security hardening — what's left after PRs 1–5

**Source plan:** `~/.claude/plans/do-security-research-in-snazzy-wreath.md` (not in the repo).
**Board cards:** #148, #149, #150, #151, #152, #153 · landed work tracked on #154.
**Goal:** finish the security-hardening plan. PRs 1–5 shipped; what remains is one operational
P0, one applied-by-hand proxy change, and the two gated rollouts.

## Applied to production 2026-07-30

| Card | State | Verified by |
|---|---|---|
| #149 per-IP throttle | **live** | 70-request parallel burst → 41×401 then 29×429; `/auth/me` 70 requests → **0** 429s; `/config/cameras` 60 → **0** 429s |
| #150 access log + rotation | **live** | canary token in a query string → **0** occurrences in the log; `token=` → **0**; 429s recorded with `RouterName: sentinel-login@docker` and `RequestPath: (absent)` |
| #153 WebAuthn RP | **fixed** | `rp_id=olharai.com origin=https://app.olharai.com`; login/start 501 → 404 |
| #151 media tokens | phase 1 deployed | see that section |
| §1.1 demo admin | **closed** | `admin@demo.local` went 200 → 401; the credential no longer authenticates |
| #148 §0 secrets | **still open** | `JWT_SECRET` rotation, the Stripe pair, and `MASTER_PASSWORD` all remain — see below |

Sequential request loops are useless for testing a token bucket: at 20/min with burst 40, 46 requests
spaced ~1s apart refill faster than they drain and produce zero 429s. Use a parallel burst.

The `RequestPath: drop` is load-bearing, not cosmetic — the canary test above is what proves it, and
it must stay until §1.8 phase 2 lands.

## Status of the original sequencing

| Step | Scope | State |
|---|---|---|
| 1. "Now, no deploy" | rotate the four §0 secrets; change/delete the demo admin | demo admin **closed**; the three secrets **still open** — card #148 |
| PR 1 | §1.1 seed, §1.2 live-start, §1.5 command-result, §1.6 RBAC + guard test | landed `a4dbe5e` |
| PR 2 | §1.3/§1.4 fail-closed, §1.7 account-half lockout | landed `036171e` (deployed only on 2026-07-29 — see below) |
| PR 3 | §3 P0 — `app_core::signing`, 0077, bootstrap, middleware | landed `602abbf` + `4f80ffc` |
| PR 4 | §3 P1 — edge signs all 10 sites | landed `b2c1a28`, released `v0.5.65` |
| PR 5 | §2 E0 — WebAuthn enrollment + optional passkey login | landed `ab1dea9` + `3ffc8b0`, **misconfigured in prod — card #153** |
| Later | §1.8 media tokens; §3 P3 enforcement; §2 E1→E2 | cards #151, #152; E1→E2 still gated |

**PR 2 spent days committed but undeployed.** Its CI run was cancelled by a following docs-only
push, whose own run matched no service and so deployed nothing. Migration 0078 did not reach prod
until 2026-07-29. Verify migrations in the live DB after any security deploy — the CI UI will not
tell you.

## #148 — §0: rotate the exposed secrets + remove the demo admin (P0, operational)

### §1.1 — CLOSED 2026-07-30

It was live. `admin@demo.local` / `demo12345` returned **200** on the public login endpoint of
`control.thesentinels.online` — a working Admin on a real tenant — exactly as the plan predicted:
the code fix made the demo tenant dev-only but **cannot delete an account that already exists**.

Closed with `UPDATE users SET password_hash = NULL WHERE email = 'admin@demo.local'` (1 row).
`password_hash IS NULL` makes `handlers::login` answer "use social login for this account" instead
of authenticating, so the password path is shut with no FK-cascade risk, and it is reversible.
Re-tested: **401**.

Two follow-ups this surfaced:

- The `demo` tenant and its user still exist, now unauthenticatable. Deleting the tenant outright is
  the fuller cleanup.
- `"use social login for this account"` differs from the message for an unknown address, so it
  confirms an account exists — a small enumeration oracle. Pre-existing behaviour for OAuth-only
  accounts, not introduced here, but it is inconsistent with the care taken in the lockout path,
  which deliberately returns an identical message for a wrong password and an unknown address.

Four live secrets were printed into a session transcript during the original review:
`JWT_SECRET`, `MASTER_PASSWORD`, `STRIPE_API_KEY`, `STRIPE_WEBHOOK_SECRET`. **No commit can fix
this** — it is a rotation, done on the VPS in `/ibbf/sentinel.env`.

Split by who can actually do each one:

- **`JWT_SECRET`** — rotatable on the box, but it invalidates every session (all users logged out;
  appliances unaffected). Must be written **once** and both `sentinel-control` and
  `sentinel-gateway` recreated together, or the gateway silently rejects every live-video token.
  Generate it server-side and never echo it:
  ```sh
  cd /ibbf && cp -n sentinel.env sentinel.env.bak-jwt
  NEW=$(openssl rand -base64 72 | tr -d '\n')
  sed -i "s|^JWT_SECRET=.*|JWT_SECRET=${NEW}|" sentinel.env && unset NEW
  grep -c '^JWT_SECRET=' sentinel.env   # expect 1; do NOT print the value
  docker compose -f docker-compose.yml -f docker-compose.sentinel.yml up -d sentinel-control sentinel-gateway
  ```
- **`STRIPE_API_KEY` / `STRIPE_WEBHOOK_SECRET`** — cannot be rotated from the box at all; they are
  rolled in the Stripe dashboard, then pasted into `sentinel.env`. §0 is **not closed** until this
  happens: PR 2 shipped the webhook verification that makes the leaked signing secret meaningful.
- **`MASTER_PASSWORD`** — deliberately left for the operator. Setting it would require telling them
  the new value, and writing a live credential into a session transcript is precisely the mistake
  that created §0 in the first place.

The plan required rotating `JWT_SECRET` and `STRIPE_WEBHOOK_SECRET` **before** shipping §1.3/§1.4.
Those shipped and are now deployed, so the ordering has already been violated: if rotation has not
happened, an exposed `JWT_SECRET` is live and can mint `master: true` sessions for any tenant.

- Rotating `JWT_SECRET` logs every user out; **appliances are unaffected** (box tokens are opaque
  bearers hashed against `tenant_boxes`, not JWTs).
- `sentinel-gateway` must get the identical value or it silently rejects every live-video token.
  Verified 2026-07-30 that both containers read the same `sentinel.env` and agreed on the value, and
  that only control and gateway reference `JWT_SECRET` in code — flow and notification inherit the
  file but never use it, so they do not need recreating.

## #149 — §1.7 IP half: per-IP throttle at the proxy

**LIVE as of 2026-07-30.** Committed in `ca88ad6` as Traefik labels on `sentinel-control` in
`deploy/vps/docker-compose.sentinel.yml`, then applied by hand to `/ibbf/docker-compose.sentinel.yml`
— deploy never overwrites that file (it holds substituted secrets), so the repo copy is documentation
of intent and the VPS copy is what runs. Re-applying after any future edit is a manual step.

Shape: a separate router matching only `/auth/login`, `/auth/signup` and
`/auth/webauthn/login/*`, at 20/min with burst 40, no `ipStrategy` (DNS points straight at the
host, so Traefik's remote address is the client). It must not hang off the catch-all router, or it
would also throttle `/auth/me` and the edge's `/config/*` long-polls. The rule covers
`control.olharai.com` as well as `control.thesentinels.online`.

## #150 — Traefik access logging + container log rotation

**LIVE as of 2026-07-30**, in `/ibbf/traefik.yml` with `fields.names.RequestPath: drop`.

That one line is the whole safety of this change: **Traefik's `RequestPath` includes the query
string**, and #151 phase 2 has not landed, so logging it would write full session JWTs to disk.
Filtering to 4xx/5xx does not help — a 401 on a media or SSE URL is exactly the request carrying a
token. Proven by sending `?token=CANARY…` and grepping the log: zero occurrences, and zero `token=`
anywhere. `ClientHost`, `DownstreamStatus`, `RouterName` and `StartUTC` are enough to see who was
refused and how often. **Do not restore `RequestPath` before #151 phase 2.**

Also add `logging:` options to the `traefik` service — there is no `/etc/docker/daemon.json`, so
json-file keeps everything. Static config, so it needs a Traefik restart, which briefly affects
every site on the box.

## #151 — §1.8: scoped media tokens (get JWTs out of query strings)

Full 24h session tokens still travel in URLs, because `<img>` and `EventSource` cannot set
headers: `sentinel-app/src/lib/data.ts:137` (`/workflows/events?token=`) and the gateway's
`?token=` (`sentinel-gateway/src/auth.rs:55`). Browser history and `Referer` are live vectors
today; access logs become one the moment #150 lands.

**Phase 1 landed.** `Claims.scope` (`#[serde(default)]` + `skip_serializing_if`, so tokens minted
before the field existed still decode and session tokens stay byte-identical in shape),
`POST /auth/media-token`, and `auth::verify_media` on the four control surfaces.

The property that matters is **not** the shorter expiry — it is that `UserClaims` **refuses** a
media token from day one, so one that leaks via history, `Referer`, or an access log cannot call the
API at all. `UserClaims` also guards the mint endpoint, which means a media token cannot mint
another one: their lifetime is bounded by the session that created them. Media surfaces still accept
session tokens during the overlap (phase 2 removes that), and each such use is logged so phase 2 can
be scheduled on evidence.

**TTL is one hour, not the 120s the plan proposed** (`MEDIA_TOKEN_TTL_SECS` overrides). 120s is
right for a one-shot `<img src>` and wrong for this product: dashboards sit open all day, and a
native `EventSource` retries the *same* URL it was constructed with, so a 2-minute token would force
a reconnect every 2 minutes for as long as the tab is open. An hour is still 24× tighter than the
24h session token it replaces.

App side: `mediaToken()` in `auth.svelte.ts` caches one and re-mints with five minutes to spare;
primed on boot (`loadMe`) and on every `setToken`, and cleared on sign-out and "act as tenant" so a
stale token from the previous identity cannot outlive it. The three URL builders in `data.ts`
(`faceImageUrl`, `runFrameUrl`, `nodeStateUrl`) use it, falling back to the session token if a mint
has not landed yet. `Referrer-Policy: no-referrer` added in `deploy/app/nginx.conf`.

**Deliberately NOT in phase 1 — two surfaces still carry session tokens:**

- **The SSE stream** (`data.ts` `ensureCloudStream`). A media token would work, but only by relying
  on a 401 → `CLOSED` → 3-second manual retry once an hour, i.e. an hourly gap that does not exist
  today. Doing it properly means re-minting and reconnecting *before* expiry. Worth doing; not worth
  smuggling into this change.
- **The gateway's `?token=`** (`sentinel-gateway/src/auth.rs`, used by the live player). The gateway
  has its own `Claims` copy and would need the same scope handling; live video is the core product,
  so it gets its own change with its own verification.

**Phase 2** (refuse session tokens on media surfaces) is gated on the phase-1 log going quiet, and
on both surfaces above moving first — otherwise it breaks live video and the event stream.

## #152 — §3 P2→P3: soak, then enforce signing per box

All the signing machinery is live and protecting nothing: every box is `signing_mode='off'` with
`last_signed_at` null, correct while the fleet ran 0.5.64. `v0.5.65` (which signs all ten
call sites) is now released, so the P2 soak can begin.

Advance a box to `'required'` only when `last_signed_at` < 10 min **and** `last_unsigned_at` is
more than 24h old. The second condition is the real gate: it proves *every* code path signs,
including rarely-firing ones like clip-confirm and disk-alert. Flip in batches — internal, then
one friendly tenant, then the rest. Never auto-demote a box out of `required` on 401s; that is a
downgrade oracle.

Fleet as of 2026-07-30: Silicom Appliance (Ubuntu x86_64, 0.5.64) and CoralDEV (Mendel aarch64,
0.5.52) can take the Linux 0.5.65 build; **Jack Mac (macOS) cannot** — `release.yml` builds Linux
only since `7351f42`, so macOS needs the out-of-band signed + notarized `.pkg`. Leave boxes that
cannot reach 0.5.65 on `'off'` as reported security debt rather than stranding them.

## #153 — WebAuthn RP ID unset in production (bug)

`WEBAUTHN_RP_ID` and `WEBAUTHN_RP_ORIGIN` are not set in `/ibbf/sentinel.env`, so
`sentinel-control/src/webauthn.rs` fell back to its defaults — RP ID `localhost`, origin
`http://localhost:5173`. The endpoints are live (`/auth/webauthn/login/start` answers 404, not
501), so passkeys are **live and broken** rather than cleanly disabled: a browser on
`app.olharai.com` will refuse the assertion on RP-ID mismatch.

The module's own comment claims a bad config makes every endpoint answer 501 and the app degrades
to password login. That is not what *unset* env does — it produces a working WebAuthn instance
pointed at localhost. Worth making the fallback release-hostile as well as setting the vars.

## Out of scope here

- §2 E1 (enroll-after-login nudge) and E2 (enforcement) — gated on a working recovery path and
  mobile WebAuthn landing; they ship as their own cards. See docs/plans/fido2-webauthn.md.
- The v1 nonce store (skew window only, by design) and the `send_json` CI guard, which `b2c1a28`
  already added.
