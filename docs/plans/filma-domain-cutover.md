# Filma AI domain cutover — filma.ai (+ .com.br) alongside thesentinels.online

**Board card:** #99 Domain cutover — serve filma.ai / .com.br, transitional dual-domain (ToDo)
**Parent epic:** [[filma-rebrand]] (#76) — this is its Phase 1 (infra), the risky half. Phase 3
(the visible copy) already shipped as "Filma AI".
**Goal:** The VPS answers on **filma.ai** and **filma.ai.br** as well as the current
`thesentinels.online`, both live during a transition, with nothing broken on day one — no dead
OAuth redirects, no failed Stripe returns, no broken bookmarks. `thesentinels.online` 301s to
`filma.ai` only once the transition is done.

Requested 2026-07-17: *"change the VPS to receive access on Filma AI.com and .com.br"*, plus the
landing page (tracked as its own card — see Out of scope).

---

## Why this is the dangerous phase (and Phase 3 wasn't)

The name in copy is a string. A **domain** is wired into auth and payments: OAuth registers exact
redirect URIs, Stripe stores exact success/cancel URLs, and browsers hold bookmarks and live
sessions. Flip the host without preparing those and users mid-signup or mid-checkout hit a wall. So
this is **additive dual-domain first**, cutover later — never a hard switch.

The repo already has the precedent, and it's exactly this shape (`deploy/vps/docker-compose.sentinel.yml`):

```
traefik...sentinel-gw.rule=Host(`gateway.thesentinels.online`) || Host(`gateway.ibbf.life`)
```

The gateway already serves a legacy domain via `||`. Every router follows that pattern for the new
domains; nothing is replaced.

## Inventory (23 hardcode sites, from the epic + confirmed today)

| Where | What | Action |
|---|---|---|
| `deploy/vps/.../sentinel.yml` — Traefik `Host()` for control/app/landing/gateway | routing | add `\|\| Host(\`*.filma.ai\`)` per router; keep the old |
| same — `OAUTH_REDIRECT_BASE` | `https://control.thesentinels.online` | new value once the OAuth app has the new URI registered (below) |
| same — `STRIPE_SUCCESS_URL` / `STRIPE_CANCEL_URL` | app URLs | new once Stripe has them |
| `.env.example` | the Stripe URLs (docs) | mirror |
| `sentinel-app/src/lib/config.ts` | `hostname.endsWith("thesentinels.online")` picks the control/gateway base | **must also match `filma.ai`**, or the app loads on the new domain and calls the old API |
| `sentinel-app/src/app.html` | OG/Twitter image URLs | new host (with og.png, still pending in #76) |
| `crates/app-core/src/billing/payments/stripe.rs` | `success_url`/`cancel_url` defaults | new |

The frontend one (`config.ts`) is the sharp edge: it resolves which control/gateway to call by
sniffing its own hostname. On `filma.ai` today it would fall through to the localhost default and
the app would silently talk to nothing. That check must learn the new domain **before** the domain
serves the app.

## TLS + DNS

- DNS on **Cloudflare** (matching the existing domains), for `filma.ai`, `filma.ai.br`, and the
  `app.` / `control.` / `gateway.` subdomains.
- Traefik gets certs for the new hosts (same ACME resolver already issuing for `*.thesentinels.online`).
- **`filma.ai.br` is a defensive 301 to `filma.ai`**, not a localized site — a Traefik redirect
  router, no app behind it.

## Manual, off-box steps (flag, don't automate — they gate the deploy)

These are external consoles and must be done *before* the redirect-base env values flip, or auth and
payments break the moment the new domain is primary:

1. **OAuth provider** — register `https://control.filma.ai/...` as an allowed redirect URI
   (alongside the current one, both valid during transition).
2. **Stripe** — add the `app.filma.ai` success/cancel URLs.
3. **Cloudflare** — the zones + records above.
4. Any other consumer of the callback (magic-link emails, etc.) — audit for a hardcoded host.

## Order (so nothing is ever broken)

1. DNS + certs for the new hosts (they resolve, TLS valid, still routing to the same services).
2. `config.ts` learns `filma.ai` — ship the app so it calls the right API on either domain.
3. Traefik `Host()` gains the new domains via `||` — both live.
4. Register the new OAuth/Stripe URLs (manual), keeping the old.
5. Flip `OAUTH_REDIRECT_BASE` / Stripe env to the new host once step 4 is confirmed.
6. `.com.br` → `.com` 301.
7. **Later**, when traffic has moved: `thesentinels.online` 301 → `filma.ai`, retire the old
   redirect URIs.

## Verification

- Sign in on `app.filma.ai` end to end — the OAuth round-trip returns to the new host.
- A Stripe checkout from `app.filma.ai` returns to its success URL.
- `thesentinels.online` still fully works throughout (no premature cutover).
- `filma.ai.br` 301s to `filma.ai`.
- The app on the new domain calls the new control/gateway, not localhost (the `config.ts` trap).

## Out of scope

- **Landing page rewrite** — its own card [[filma-landing]] (#100). The epic notes the current copy
  "doesn't reflect the power of the app"; that's a content rewrite, not a routing change, and it has
  its own `landing/` build with its own copy of the mark to rebrand.
- Repo/service/container names stay `sentinel-*` — this changes what users reach, not the codebase.
- `og.png` regeneration (needs the lockup rendered with Sora) — tracked in #76.
