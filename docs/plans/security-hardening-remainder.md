# Security hardening — what's left after PRs 1–5

**Source plan:** `~/.claude/plans/do-security-research-in-snazzy-wreath.md` (not in the repo).
**Board cards:** #148, #149, #150, #151, #152, #153 · landed work tracked on #154.
**Goal:** finish the security-hardening plan. PRs 1–5 shipped; what remains is one operational
P0, one applied-by-hand proxy change, and the two gated rollouts.

## Status of the original sequencing

| Step | Scope | State |
|---|---|---|
| 1. "Now, no deploy" | rotate the four §0 secrets; change/delete the demo admin | **card #148 — believed NOT done** |
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

Four live secrets were printed into a session transcript during the original review:
`JWT_SECRET`, `MASTER_PASSWORD`, `STRIPE_API_KEY`, `STRIPE_WEBHOOK_SECRET`. **No commit can fix
this** — it is a rotation, done on the VPS in `/ibbf/sentinel.env`.

The plan required rotating `JWT_SECRET` and `STRIPE_WEBHOOK_SECRET` **before** shipping §1.3/§1.4.
Those shipped and are now deployed, so the ordering has already been violated: if rotation has not
happened, an exposed `JWT_SECRET` is live and can mint `master: true` sessions for any tenant.

- Rotating `JWT_SECRET` logs every user out; **appliances are unaffected** (box tokens are opaque
  bearers hashed against `tenant_boxes`, not JWTs).
- `sentinel-gateway` must get the identical value or it silently rejects every live-video token.
- Separately, §1.1's code fix made the demo tenant dev-only, but **it does not delete an account
  that already exists**. Confirm whether `admin@demo.local` is still a valid login in prod and
  remove it or change its password.

## #149 — §1.7 IP half: per-IP throttle at the proxy

Committed in `ca88ad6` as Traefik labels on `sentinel-control` in
`deploy/vps/docker-compose.sentinel.yml`. **Not live**: deploy never overwrites
`/ibbf/docker-compose.sentinel.yml` (it holds substituted secrets), so the labels must be added to
the VPS copy by hand and `sentinel-control` recreated.

Shape: a separate router matching only `/auth/login`, `/auth/signup` and
`/auth/webauthn/login/*`, at 20/min with burst 40, no `ipStrategy` (DNS points straight at the
host, so Traefik's remote address is the client). It must not hang off the catch-all router, or it
would also throttle `/auth/me` and the edge's `/config/*` long-polls. The rule covers
`control.olharai.com` as well as `control.thesentinels.online`.

## #150 — Traefik access logging + container log rotation

Traefik currently logs only `level=error`, so the throttle's 429s and every failed-login 401 are
unrecorded. Blocked on one hard constraint: **Traefik's `RequestPath` includes the query string,
and #151 is still open**, so logging it would write full 24h session JWTs to disk. Either land
#151 first, or set `fields.names.RequestPath: drop` — `ClientHost`, `DownstreamStatus`,
`RouterName` and `StartUTC` are enough to see which address was refused and how often.

Also add `logging:` options to the `traefik` service — there is no `/etc/docker/daemon.json`, so
json-file keeps everything. Static config, so it needs a Traefik restart, which briefly affects
every site on the box.

## #151 — §1.8: scoped media tokens (get JWTs out of query strings)

Full 24h session tokens still travel in URLs, because `<img>` and `EventSource` cannot set
headers: `sentinel-app/src/lib/data.ts:137` (`/workflows/events?token=`) and the gateway's
`?token=` (`sentinel-gateway/src/auth.rs:55`). Browser history and `Referer` are live vectors
today; access logs become one the moment #150 lands.

Fix: `#[serde(default)] scope: Option<String>` on `Claims` (backward-compatible), a
`POST /auth/media-token` minting 120-second `scope:"media"` tokens, required on those handlers.
Two-phase rollout — accept both, then reject session tokens — because control, gateway and app
must move together. Free partial mitigation now: `Referrer-Policy: no-referrer`.

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
