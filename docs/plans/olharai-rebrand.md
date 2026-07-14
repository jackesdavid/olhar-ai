# OlharAI Rebrand — Implementation Plan

**Board card:** Epic — OlharAI rebrand (domains, brand, app, landing copy)
**Goal:** Everything a user sees moves from **Sentinel** / `thesentinels.online` to **OlharAI** /
`olharai.com`. Everything internal — repo name, service names (`sentinel-control`,
`sentinel-app`, `sentinel-edge`, `sentinel-flow`, `sentinel-gateway`, `sentinel-landing`,
`sentinel-notification`), crate names, docker/Traefik router names, storage keys — **stays
`sentinel-*`**, by explicit instruction. This is a user-facing rename + a full landing-page copy
rewrite, not a codebase rename.

## Context

The owner bought `olharai.com` and `olharai.com.br` and wants the product's public identity to
move to OlharAI. Separately (and explicitly called out): the current landing page copy "is not
reflecting the true power of this application" — it undersells the platform and needs a real
rewrite, not just a find-replace of the brand name.

## Decisions locked in (from the user)

1. **Canonical name/casing: `OlharAI`** — one word, capital O, capital AI. Use this exact casing
   everywhere (wordmark, `<title>`, OG tags, manifest, UI copy, AI self-description).
2. **Keep the dot-field mark as-is.** The mark + Sora wordmark system (cards #68/#69) shipped
   *yesterday* — well-documented (`docs/ref/brand.md`), fully componentized
   (`SentinelLogo`/`SentinelMark`). Only the **wordmark text** changes ("Sentinel" → "OlharAI");
   the mark itself, the monochrome-ink theme, and the dot-field visual language are untouched. This
   makes the whole visual side of the rebrand mechanically cheap — one prop/string change plus a
   favicon/OG re-export from the *same* master mark.
3. **`olharai.com.br` is a defensive redirect**, not a localized site: it 301s to `olharai.com`.
   Its DNS (and `olharai.com`'s) should be hosted on **Cloudflare**, matching how the existing
   domains (`thesentinels.online`, and apparently others — "like the other sentinels") are already
   set up.
4. **Transitional dual-domain cutover.** Both `thesentinels.online` and `olharai.com` resolve
   during a transition window — this repo already has working precedent for exactly this
   (`deploy/vps/docker-compose.sentinel.yml`'s Traefik gateway router already does
   `Host(\`gateway.thesentinels.online\`) || Host(\`gateway.ibbf.life\`)` for a legacy domain).
   `thesentinels.online` eventually 301-redirects to `olharai.com` once the transition is done; no
   hard cutover, no broken bookmarks/OAuth-redirects/Stripe-URLs on day one.

## Current state (grounding — full inventory, gathered this session)

**Domain hardcoded in ~15 places:** `sentinel-app/src/lib/config.ts` (control/gateway base-URL
resolution via `hostname.endsWith("thesentinels.online")`), `sentinel-app/src/app.html` (OG/Twitter
image URLs), `crates/app-core/src/billing/payments/stripe.rs` (Stripe `success_url`/`cancel_url`
defaults), `.env.example` (`STRIPE_SUCCESS_URL`/`STRIPE_CANCEL_URL`), `deploy/vps/docker-compose.sentinel.yml`
(Traefik `Host()` router rules for control/app/landing/gateway, `OAUTH_REDIRECT_BASE`,
`OAUTH_SUCCESS_REDIRECT`), `sentinel-edge/install.sh`, `docs/ref/deploy.md`, e2e test
fixtures/config (`sentinel-app/e2e/seed.ts`, `playwright.config.ts`, `.env.test.example`,
`recordings.spec.ts`, `workflow-wire-bend.spec.ts`, `e2e/.auth/session.json`).

**No Terraform/IaC for DNS** — it's managed directly at the registrar/Cloudflare, not in this repo.
TLS is automatic: Traefik issues Let's Encrypt certs per `Host()` rule label, so adding
`olharai.com` hostnames to the existing router rules (dual-`Host()`, matching the `ibbf.life`
precedent) is enough for certs to provision themselves — no separate cert-management step.

**Brand identity (already built, card #68/#69, 2026-07-12):** `SentinelMark`
(`sentinel-app/src/lib/components/sentinel-mark.svelte`) is a pure procedural SVG with **no
rendered text** — untouched except its `aria-label="Sentinel"`. The actual text wordmark is a
*separate* component, `sentinel-logo.svelte` (`<span class="wordmark">Sentinel</span>`, one line)
composed into the `SentinelLogo` lockup (mark + wordmark, used in the sidebar header and login
card). Favicons/OG/PWA manifest were generated from the mark in `sentinel-app/static/` — need
regenerating with the new wordmark for the OG image (the favicon/app-icon set is mark-only, no
text, so those likely don't need touching).

**~10 literal "Sentinel" UI strings** (`sentinel-app/src/...`): `app.html` `<title>`/OG
`site_name`/`title`; `static/manifest.webmanifest` `name`/`short_name`; splash screen
("Initializing Sentinel…"); chat page prompts ("Ask Sentinel…"); several empty-state/description
strings (Boxes, admin/tenants, People, People/directory, Beacons, Billing).

**Backend: the AI literally calls itself "Sentinel" to users** —
`crates/app-core/src/agent/domain.rs`'s `DEFAULT_PERSONALITY` ("You are Sentinel, a calm, precise
AI assistant…") and `crates/app-core/src/agent/vision.rs`'s vision-model system prompt ("You are
Sentinel, a calm home + business monitoring assistant…") — both need their brand reference updated
so the assistant's own self-description matches. Also a literal Telegram test message
(`sentinel-control/src/domain/beacons.rs`: `"Sentinel test ✓ — delivery confirmed."`).

**No terms/privacy pages exist yet** — nothing to migrate there. **No mobile/native app** — web
only, so no app-store listing concerns.

**The landing page is tiny and copy-only-editable:** `landing/src/routes/+page.svelte` (~310
lines) is the *entire* site — hero, features, how-it-works, privacy block, final CTA, footer, SEO
meta — all inline, no CMS. The current narrative is 100% "calm home security assistant" framed
around face recognition + alerting, and **badly undersells the real platform**: the visual
workflow-automation graph editor (100+ node kinds), multi-object detection/tracking with
line-crossing/zone/dwell-time triggers (any of 80 COCO classes, not just people), the Telegram bot
as a first-class interface, AI vision reasoning (ask-your-cameras-questions via Claude), the live
map/floorplan + presence tracking view, and recording management are **all absent or reduced to a
throwaway bullet**. There's no pricing section at all.

## Phase 1 — Domain & infra

- Add `olharai.com` + `olharai.com.br` to Cloudflare (matching the existing `thesentinels.online`
  setup — nameserver delegation at the registrar → Cloudflare).
- DNS records for `olharai.com`: root (`sentinel-landing`), `app.`, `control.`, `gateway.` — same
  A/CNAME shape as today's `thesentinels.online` records, pointed at the same Contabo VPS IP.
- `olharai.com.br`: a single record + a Cloudflare **Bulk Redirect** (or Page Rule) 301-ing
  everything to `olharai.com` — no VPS-side routing needed for it at all.
- `deploy/vps/docker-compose.sentinel.yml`: extend every Traefik router's `Host()` rule to match
  BOTH domains during the transition (`Host(\`app.thesentinels.online\`) ||
  Host(\`app.olharai.com\`)`, etc.) — exactly the `gateway.ibbf.life` pattern already in this file.
  Confirm Let's Encrypt issues certs for the new hostnames on first request (Traefik's ACME
  resolver does this automatically off the router rule).
- Update the hardcoded domain in: `config.ts` (accept either suffix, prefer `olharai.com` as the
  canonical fallback target), `app.html` OG/Twitter URLs, `stripe.rs` + `.env.example`
  (`STRIPE_SUCCESS_URL`/`CANCEL_URL`), `OAUTH_REDIRECT_BASE`/`OAUTH_SUCCESS_REDIRECT` in the
  compose file, `sentinel-edge/install.sh`.
- Leave e2e test domain refs (`e2e-sentinel@thesentinels.online`, `playwright.config.ts`, etc.) for
  a later pass — not user-facing, not urgent, but note them so they don't silently rot.
- **Once the transition window ends:** flip `thesentinels.online`'s Traefik rule to a redirect-only
  router (301 → the equivalent `olharai.com` host) and retire the dual-`Host()` rules.

## Phase 2 — Brand identity: wordmark swap (mark unchanged)

- `sentinel-logo.svelte`: change the wordmark string `"Sentinel"` → `"OlharAI"`.
- `sentinel-mark.svelte`: update `aria-label="Sentinel"` → `"OlharAI"`.
- Regenerate the **OG image** (`og.png`) from the updated `SentinelLogo` lockup (new wordmark) —
  reuse #69's existing export pipeline (headless-Chrome render of the lockup), no new tooling.
  Favicons/app-icons are mark-only (no text) — verify with a visual check, but likely untouched.
- `static/manifest.webmanifest`: `name`/`short_name` → "OlharAI".
- `app.html`: `<title>`, `og:site_name`, `og:title` → "OlharAI".
- Rewrite `docs/ref/brand.md`'s prose to reference OlharAI as the product name (mark/theme
  sections stay factually the same, just the name references change).
- Nice-to-have, not blocking: update the `impeccable` skill doc's header/description (currently
  says "Design-excellence skill for the Sentinel web app") to say OlharAI, so future design work
  doesn't keep teaching the old name — the dot-field/monochrome-ink *design language* it teaches is
  unchanged, only the product-name references in its prose.

## Phase 3 — App surface copy sweep

Update every literal "Sentinel" user-facing string found this session:
- Splash screen copy ("Initializing Sentinel…").
- Chat page prompts/placeholders ("Ask Sentinel…", "Ask Sentinel about your cameras…").
- Empty-state/description copy: Boxes settings, admin/tenants, People, People/directory, Beacons,
  Billing page + `billing.svelte.ts`.
- **The AI's own self-description**: `agent/domain.rs`'s `DEFAULT_PERSONALITY` and
  `agent/vision.rs`'s system prompt — both currently say "You are Sentinel…"; change to "You are
  OlharAI…" so the assistant's actual chat responses match the new brand (this is a behavior
  change, not just cosmetic — verify by asking the chat "what's your name" post-deploy).
- The literal Telegram test message in `beacons.rs` ("Sentinel test ✓…").
- Low-priority/optional: the `localStorage` key `"sentinel-theme"` — purely internal storage, not
  user-visible; fine to leave as-is or rename opportunistically, not worth a dedicated migration.

## Phase 4 — Landing page: full copy rewrite

Rewrite `landing/src/routes/+page.svelte` (and its `<svelte:head>` SEO meta + footer tagline) from
scratch, leading with what the platform *actually* does instead of a generic "calm home security"
pitch. Concretely surface, at minimum:
- The **visual workflow-automation engine** as a headline feature, not a buried bullet — a
  real node-based graph editor (cameras, AI vision, alerts, Telegram, recordings, variables, loops,
  events), not just "drag-and-drop rules."
- **Detection + multi-object tracking** — any of 80 object classes (not just people), line-crossing
  / zone / dwell-time triggers, enter/exit events — currently completely unmentioned.
- **AI vision reasoning** — ask-your-cameras-questions (describe a scene, yes/no routing,
  extraction) — a genuinely differentiated capability, currently absent.
- **Telegram as a first-class interface** — not just a notification channel, a two-way bot brain.
- The **live map/floorplan + presence view** — currently reduced to a footnote.
- **Recording management** and the **multi-tenant / self-hosted-edge-plus-cloud** architecture
  story (framed for scale/enterprise credibility, not just "runs on your own edge").
- Keep the existing privacy framing (on-prem video, tenant isolation) — it's a real, good
  differentiator, just needs to sit alongside the rest instead of being the *only* story.
- Decide whether to add a pricing section (currently absent entirely) — flagged as an open
  decision below, not assumed.
- Swap the `SentinelLogo`/mark reference to the new wordmark (falls out of Phase 2 automatically
  once the shared component updates).
- Use the `frontend-design` skill for any visual/layout changes alongside the copy, and the
  (updated) `impeccable` skill to keep it coherent with the existing dot-field/monochrome-ink
  identity — this is a copy + polish pass on the existing single-file page, not a rebuild.

## Phase 5 — External / manual steps (outside the codebase — flagging, not executing)

- Cloudflare: add both domains, set up DNS records + the `.com.br` bulk redirect (as above).
- Registrar: point nameservers to Cloudflare for both domains.
- **Telegram @BotFather**: if you want the bot's own display name/bio (configured externally, not
  in code — no `setMyName`/`setMyDescription` calls exist in this repo) to say OlharAI, that's a
  manual step via @BotFather, not something I can do from the codebase.
- Stripe Dashboard: if the Stripe account/product display name shows "Sentinel" anywhere a customer
  sees it during checkout, update it there (separate from the `success_url`/`cancel_url` code
  change in Phase 1).
- Any OAuth app listing name (if Google/other OAuth consent screens show a product name) — check
  and update if applicable.

## Open decisions (grill before building)

1. **Pricing section on the landing page** — add one now as part of the copy rewrite, or keep
   deferring it (there's already a "Pricing simulator" card, #2, in the backlog)? No pricing
   section exists today either way.
2. **Transition window length** — how long do both domains stay live before `thesentinels.online`
   starts 301-redirecting? (Days? Weeks? Until some usage threshold drops?) No technical
   constraint either way — purely a business call.
3. **`docs/HANDOFF.md`/other internal docs** mention `thesentinels.online` historically — these are
   internal session logs, not user-facing, so per the "internal stays sentinel" rule they don't
   need editing; confirming that's the intended scope boundary (docs *about* the code stay as
   historical record; only currently-live user-facing surfaces change).

## Out of scope (v1)

- Any codebase/repo/service rename (`sentinel-control`, `sentinel-app`, etc. all stay as-is,
  explicit instruction).
- A new/redesigned mark (locked: keep the dot-field).
- PT-BR localization of the landing page or app (`.com.br` is redirect-only for now).
- Terms/privacy pages (none exist yet; not part of this rebrand).
- e2e test fixture domain updates (noted, deferred — not user-facing).
- Email template system's branding (card #70, `docs/plans/email-template-system.md`, is a separate
  planned effort — no email templates exist yet to rebrand).

## Verification

- Both domains resolve and serve the correct services (app/control/gateway/landing) with valid TLS
  during the transition window.
- `grep -ri "sentinel"` over `sentinel-app/src`, `landing/src`, and the two AI system-prompt files
  turns up nothing user-facing left unrenamed (internal identifiers excluded).
- Chat: asking the AI assistant "what's your name?" gets an OlharAI answer, not Sentinel.
- OG/Twitter link preview (a share-debugger tool) shows the OlharAI wordmark + correct title/domain.
- New landing copy reviewed against the real feature list above — every headline capability from
  Phase 4's list is represented somewhere on the page.
- `olharai.com.br` redirects cleanly to `olharai.com` (any path).
