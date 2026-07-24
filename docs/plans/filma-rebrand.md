# Filma AI Rebrand — Implementation Plan

**Board card:** Epic — Filma AI rebrand (domains, brand, app, landing copy)
**Goal:** Everything a user sees moves from **Sentinel** / `thesentinels.online` to **Filma AI** /
`filma.ai`. Everything internal — repo name, service names (`sentinel-control`,
`sentinel-app`, `sentinel-edge`, `sentinel-flow`, `sentinel-gateway`, `sentinel-landing`,
`sentinel-notification`), crate names, docker/Traefik router names, storage keys — **stays
`sentinel-*`**, by explicit instruction. This is a user-facing rename + a full landing-page copy
rewrite, not a codebase rename.

## Context

The owner bought `filma.ai` and `filma.ai.br` and wants the product's public identity to
move to Filma AI. Separately (and explicitly called out): the current landing page copy "is not
reflecting the true power of this application" — it undersells the platform and needs a real
rewrite, not just a find-replace of the brand name.

## Decisions locked in (from the user)

1. **Canonical name: `Filma AI`** — two words (settled 2026-07-17, from BRAND.md, which supersedes
   the earlier one-word lean). Used in `<title>`, OG/Twitter, manifest, UI copy and the AI's
   self-description. The wordmark lockup stays `FILMA AI` (uppercase, tracked); **`OAI` is the
   short submark for tight spaces**, not a separate brand.
2. **~~Keep the dot-field mark as-is.~~ SUPERSEDED 2026-07-16 — there is a new mark.** The owner
   supplied a complete Filma AI identity in `assets/` (8 SVGs). It is **not** the dot-field: it's an
   **eclipse/eye** — a dotted orbit ring, a disc eclipsed by an offset circle to cut a crescent, a
   pupil, and a "moon" dot riding the ring. So the visual side is no longer "one string change":
   **the mark itself is replaced**, and it becomes the *largest* slice of this epic rather than the
   cheapest. See [Phase 2](#phase-2--brand-identity-the-new-mark).

   The one thing that carries over: the orbit ring is **dotted** (`stroke-dasharray="0.1 7.754"` +
   round caps), so the new mark *evolves* the dot-field language rather than discarding it. The
   monochrome-ink theme is untouched — the supplied art is exactly our `#171717`/`#FAFAFA`.
3. **`filma.ai.br` is a defensive redirect**, not a localized site: it 301s to `filma.ai`.
   Its DNS (and `filma.ai`'s) should be hosted on **Cloudflare**, matching how the existing
   domains (`thesentinels.online`, and apparently others — "like the other sentinels") are already
   set up.
4. **Transitional dual-domain cutover.** Both `thesentinels.online` and `filma.ai` resolve
   during a transition window — this repo already has working precedent for exactly this
   (`deploy/vps/docker-compose.sentinel.yml`'s Traefik gateway router already does
   `Host(\`gateway.thesentinels.online\`) || Host(\`gateway.ibbf.life\`)` for a legacy domain).
   `thesentinels.online` eventually 301-redirects to `filma.ai` once the transition is done; no
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
`filma.ai` hostnames to the existing router rules (dual-`Host()`, matching the `ibbf.life`
precedent) is enough for certs to provision themselves — no separate cert-management step.

**Brand identity (built as #68/#69 on 2026-07-12; the mark is now being replaced — see Phase 2):**
`SentinelMark` (`sentinel-app/src/lib/components/sentinel-mark.svelte`) is a pure procedural SVG
with **no rendered text**, exposing `size`/`animated`/`rings`/`speed`/`boundary` and drawing in
`currentColor`. **Eight call sites depend on that API** — `loader`, `busy`, `splash`, `empty-state`,
`voice-orb`, `sentinel-logo`, `+layout`, and the component itself — and most use it for *motion*,
not as a logo. That is why the new mark has to be ported into this component rather than dropped in
as a file (Phase 2a).

The text wordmark is a *separate* component, `sentinel-logo.svelte`
(`<span class="wordmark">Sentinel</span>`), composed into the `SentinelLogo` lockup (sidebar header,
login card) — it loads Sora via `--font-brand`.

~~The favicon/app-icon set is mark-only, so those likely don't need touching.~~ **Wrong, now that
the mark changes: every artifact in `sentinel-app/static/` is the old mark and must be re-cut**
(Phase 2f).

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

- Add `filma.ai` + `filma.ai.br` to Cloudflare (matching the existing `thesentinels.online`
  setup — nameserver delegation at the registrar → Cloudflare).
- DNS records for `filma.ai`: root (`sentinel-landing`), `app.`, `control.`, `gateway.` — same
  A/CNAME shape as today's `thesentinels.online` records, pointed at the same Contabo VPS IP.
- `filma.ai.br`: a single record + a Cloudflare **Bulk Redirect** (or Page Rule) 301-ing
  everything to `filma.ai` — no VPS-side routing needed for it at all.
- `deploy/vps/docker-compose.sentinel.yml`: extend every Traefik router's `Host()` rule to match
  BOTH domains during the transition (`Host(\`app.thesentinels.online\`) ||
  Host(\`app.filma.ai\`)`, etc.) — exactly the `gateway.ibbf.life` pattern already in this file.
  Confirm Let's Encrypt issues certs for the new hostnames on first request (Traefik's ACME
  resolver does this automatically off the router rule).
- Update the hardcoded domain in: `config.ts` (accept either suffix, prefer `filma.ai` as the
  canonical fallback target), `app.html` OG/Twitter URLs, `stripe.rs` + `.env.example`
  (`STRIPE_SUCCESS_URL`/`CANCEL_URL`), `OAUTH_REDIRECT_BASE`/`OAUTH_SUCCESS_REDIRECT` in the
  compose file, `sentinel-edge/install.sh`.
- Leave e2e test domain refs (`e2e-sentinel@thesentinels.online`, `playwright.config.ts`, etc.) for
  a later pass — not user-facing, not urgent, but note them so they don't silently rot.
- **Once the transition window ends:** flip `thesentinels.online`'s Traefik rule to a redirect-only
  router (301 → the equivalent `filma.ai` host) and retire the dual-`Host()` rules.

## Phase 2 — Brand identity: the new mark

**Input:** 8 SVGs in `assets/` (untracked as of writing — commit them as the masters):

| File | viewBox | What it is |
|---|---|---|
| `olhar-ai-mark-black.svg` / `-white.svg` | 120×120 | the bare mark, ink / paper |
| `olhar-ai-app-icon.svg` | 120×120 | mark on an `rx=14` ink tile |
| `olhar-ai-mark-animated.svg` | 120×120 | app-icon tile + `.orbit` / `.moon` / `.pupil` hooks |
| `olhar-ai-lockup-black.svg` / `-white.svg` | 480×120 | mark + `FILMA AI`, Sora 600, tracking 7.5 |
| `olhar-ai-oai-black.svg` / `-white.svg` | 240×120 | mark + `OAI` short lockup |

The geometry is consistent across all 8 (same ring r=50, disc r=31, eclipse at 71,49 r=27, pupil
r=6, moon at 95.4,24.6) — it's one mark, correctly drawn. What follows is what has to happen
*around* it before it can ship.

### 2a. The mark must become a component, not a file

This is the real work, and the plan previously missed it. `SentinelMark`
(`sentinel-app/src/lib/components/sentinel-mark.svelte`) is **procedural SVG with a behavioural
API** — `size`, `animated`, `rings`, `speed`, `boundary`, all `currentColor`. Eight call sites lean
on that API, and most want *motion*, not a logo: `loader.svelte`, `busy.svelte`, `splash.svelte`,
`empty-state.svelte`, `voice-orb.svelte`, `sentinel-logo.svelte`, `+layout.svelte`.

A static `<img src="mark.svg">` serves none of them. So: **port the new geometry into
`sentinel-mark.svelte`**, keeping the prop surface so the call sites don't change. Map the old
concepts onto the new form:

- `boundary` → the app-icon's ink tile (the old rounded-hex perimeter's role).
- `animated` + `speed` → the orbit/moon motion (see 2c), replacing the radial "ping".
- `rings` → likely meaningless now (the dot-field had concentric rings; this has one orbit).
  Decide: drop the prop, or repurpose it as orbit-dot density. **Don't silently ignore it.**

### 2b. Outline the wordmark text — the lockups are wrong as supplied

All three lockups render the wordmark as a live `<text font-family="Sora, sans-serif">`. That only
looks right where Sora is loaded. As standalone SVG assets they will be **used exactly where it
isn't**: favicon, OG image, email, README, any `<img>` embed, anyone's design tool. There the text
silently falls back to the system sans — wrong tracking, wrong weight, wrong brand.

**Convert the text to paths** for every standalone asset. Keep live text only inside
`sentinel-logo.svelte`, where the app already loads Sora via `--font-brand` (that component
composes mark + `<span class="wordmark">`, so it stays text and stays accessible).

Note the supplied lockups say **`FILMA AI`** — all-caps, spaced — while decision #1 locks the
canonical name as **`Filma AI`** (one word). Those are different marks of the same brand and that's
legitimate (a wordmark may be styled), but `sentinel-logo.svelte` currently renders sentence-case
text. **Resolve before building** — see Open decisions.

### 2c. `mark-animated.svg` has hooks but no animation

It declares `.orbit`, `.moon`, `.pupil` and defines **no motion at all** — no `<style>`, no
`@keyframes`, no `<animate>`. As shipped it's identical to the app icon. The classes say the intent
(rotate the ring, travel the moon, maybe a blink), but the motion has to be written. The old mark's
"calm ambient ping" with a `speed` prop is the bar; whatever we write should honour
`prefers-reduced-motion`, which a logo that animates constantly must.

### 2d. Fold black/white into `currentColor`

The art hardcodes `#171717` / `#FAFAFA` in paired files. Our system uses one `currentColor`
component and lets the theme decide — that's why there's no "dark logo" today. Collapsing the pairs
means 3 masters instead of 6, and light/dark keeps working for free. Keep the fixed-colour variants
only as the **exported** artifacts (email, OG, third parties) where `currentColor` can't resolve.

### 2e. `mask id="ecl"` collides when inlined

Every file hardcodes `id="ecl"`. Inline two of them in one document — mark in the sidebar, mark in
a loader — and the duplicate id makes the second mask reference the first. Ids must be generated
per-instance (`$props.id()` / a counter), which the procedural component gets to do naturally.

### 2f. Re-export the raster set

`sentinel-app/static/` holds #69's output: `favicon.ico`/`-16`/`-32`, `favicon.svg`,
`apple-touch-icon.png`, `icon-192`/`-512` (+ `-maskable`), `og.png` (1200×630),
`manifest.webmanifest`. **All of it is the old mark** and must be re-cut from the new one.
`.claude/skills/logo-designer/scripts/export.sh` is the existing pipeline — reuse it.

Two traps:
- **Maskable icons**: `app-icon.svg` bakes its own `rx=14` tile. Android/iOS apply *their own* mask,
  so a baked corner radius double-rounds and the art overruns the safe zone. Maskable variants need
  the mark at ~80% within a full-bleed square, no baked rounding.
- **Favicon at 16px**: the mark's fine details — the 7.75px-gap orbit dots, the r=4 moon — will mush.
  Check the 16px render; the old mark was explicitly designed to survive it (`brand.md`). A
  simplified favicon variant (drop the orbit, keep the eclipsed eye) may be needed.

### 2g. The rest of the identity swap

- `sentinel-logo.svelte`: wordmark `"Sentinel"` → the resolved Filma AI form (see 2b).
- `sentinel-mark.svelte`: `aria-label="Sentinel"` → `"Filma AI"`.
- `static/manifest.webmanifest`: `name`/`short_name` → "Filma AI".
- `app.html`: `<title>`, `og:site_name`, `og:title` → "Filma AI".
- **Rewrite `docs/ref/brand.md` properly.** It documents the dot-field mark as the identity
  ("a sensor's field of awareness… dense/bright at the centre"). That prose is now *false*, not
  merely misnamed. Rewrite the mark section around the eclipse/eye and what it means; keep the
  monochrome-ink and Sora sections, which still hold.
- `static/brand/mark.svg` (the master) → the new mark.
- Nice-to-have: the `impeccable` skill doc still teaches "dot-field" as the design language.

## Phase 3 — App surface copy sweep

Update every literal "Sentinel" user-facing string found this session:
- Splash screen copy ("Initializing Sentinel…").
- Chat page prompts/placeholders ("Ask Sentinel…", "Ask Sentinel about your cameras…").
- Empty-state/description copy: Boxes settings, admin/tenants, People, People/directory, Beacons,
  Billing page + `billing.svelte.ts`.
- **The AI's own self-description**: `agent/domain.rs`'s `DEFAULT_PERSONALITY` and
  `agent/vision.rs`'s system prompt — both currently say "You are Sentinel…"; change to "You are
  Filma AI…" so the assistant's actual chat responses match the new brand (this is a behavior
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
  in code — no `setMyName`/`setMyDescription` calls exist in this repo) to say Filma AI, that's a
  manual step via @BotFather, not something I can do from the codebase.
- Stripe Dashboard: if the Stripe account/product display name shows "Sentinel" anywhere a customer
  sees it during checkout, update it there (separate from the `success_url`/`cancel_url` code
  change in Phase 1).
- Any OAuth app listing name (if Google/other OAuth consent screens show a product name) — check
  and update if applicable.

## Open decisions (grill before building)

**Raised by the new assets (2026-07-16):**

1. **`FILMA AI` or `Filma AI`?** The supplied lockups set the wordmark as all-caps, letter-spaced
   `FILMA AI`; decision #1 locks the canonical name as `Filma AI`, and `sentinel-logo.svelte` renders
   sentence-case. A styled wordmark differing from the written name is normal (cf. the Sentinel
   lockup's own sentence-case rule in `brand.md`) — but it must be *decided*, not drifted into.
   Leaning: keep `Filma AI` in all copy/`<title>`/manifest, and treat `FILMA AI` as the wordmark
   lockup's styling only. **Owner's call — it's their art.**
2. **What is `OAI` for?** The 240×120 short lockup implies a compact context (favicon wordmark?
   collapsed sidebar? app icon?). Nothing in the plan asks for it. Confirm where it's meant to live,
   or it ships unused.
3. **Does `rings` survive?** The dot-field had concentric rings; the eclipse has one orbit. Drop the
   prop (touching every call site that passes it) or repurpose it as orbit-dot density?
4. **What motion do `.orbit`/`.moon`/`.pupil` imply?** The hooks name the intent but no animation is
   defined (2c). Ring rotation + moon travel is the obvious read; a pupil blink is possible but
   risks being gimmicky on a logo that sits in a sidebar all day. Owner's call.
5. **A simplified 16px favicon variant?** If the orbit dots mush at favicon size (2f), do we ship a
   reduced mark (eclipsed eye only) or accept the mush?


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
- Chat: asking the AI assistant "what's your name?" gets an Filma AI answer, not Sentinel.
- OG/Twitter link preview (a share-debugger tool) shows the Filma AI wordmark + correct title/domain.
- New landing copy reviewed against the real feature list above — every headline capability from
  Phase 4's list is represented somewhere on the page.
- `filma.ai.br` redirects cleanly to `filma.ai` (any path).
