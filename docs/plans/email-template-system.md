# Branded Email Template System — Implementation Plan

**Use case:** inline — expand the visual identity; "email templates don't exist yet."
**Board card:** Branded email template system (ToDo, #70)
**Goal:** A **branded, client-robust HTML email system** — a reusable email shell (logo header,
footer, button, typographic scale) + a small catalog of transactional templates rendered from Rust,
with a light/dark, mobile-safe look that matches the Sentinel identity. This is the **template
layer**; the **sending primitive** is built by `[[invite-onboarding-email]]` (#65) — the two
dovetail: #70 renders, #65 delivers.

**Coordinates with #65** (which proposes `crates/app-core/src/email/` + a Resend HTTPS sender).
Best sequenced so #70 provides the shell + first template that #65 sends. Uses `frontend-design` +
`impeccable`, constrained to **email-safe HTML** (see below).

## Current state (grounding)

- **No email infrastructure exists** anywhere — `sentinel-notification` + `app_core::notification`
  only do Telegram/beacons (`crates/app-core/src/notification/`). No SMTP/Resend/SendGrid.
- #65's plan already sketches `crates/app-core/src/email/` (mod + client + templates), env config
  (`EMAIL_API_KEY`/`EMAIL_FROM`/`APP_BASE_URL`), a `email::send(to, subject, html, text)` API, and a
  dev fallback that logs the rendered email. **#70 owns the `templates/` half of that module.**
- The logo header art comes from **#68** (a static PNG/SVG export — email clients can't run the
  procedural `SentinelMark` component).

## Email-safe HTML constraints (the hard part — respect these)

Email HTML is NOT web HTML. The shell must be built for it:
- **Table-based layout**, inline CSS (no external stylesheet, no `<link>`; a `<style>` head is
  ignored/stripped by many clients, esp. Gmail). No flexbox/grid, no CSS variables at runtime.
- Fixed max-width (~600px), fluid single column on mobile via `@media` in `<head>` (progressive
  enhancement only — must look right without it).
- **Logo as a hosted PNG** (`<img>` with absolute URL from `APP_BASE_URL`/CDN) + `alt` text; many
  clients block images by default, so the layout must read with images off. No SVG (Outlook/Gmail
  won't render it). Retina via width-constrained 2× PNG.
- **Dark mode:** `@media (prefers-color-scheme: dark)` + `[data-ozm]`-style overrides where
  supported; but pick colours that survive a client force-inverting them. Monochrome ink helps here.
- **Always ship a plaintext alternative** (deliverability + accessibility) — `send(html, text)`.
- Bulletproof buttons (VML fallback for Outlook, or padded table-cell anchors).

## Phase 1 — The email design system (shell + primitives)

- New `crates/app-core/src/email/templates/` (under #65's `email` module). A **base shell**
  renderer: header (logo + product name), content slot, footer (address, unsubscribe/manage where
  relevant, © line), and shared primitives — button, heading, paragraph, divider, code/token block
  — all as inline-styled table fragments.
- A tiny **inline-style token map** in Rust (ink palette, spacing, radius) so templates read
  consistently without runtime CSS. Match the monochrome ink identity; state-colour only where a
  template needs it (e.g. a security warning).
- **Render approach:** decide — hand-written Rust format strings vs. a compile-time template engine
  (e.g. `askama`, already-rustls-friendly, no runtime dep) vs. `maud`. Recommend **`askama`** for
  type-checked templates + partial reuse (see Open decision 1).
- Output: `render_xxx(ctx) -> (subject, html, text)`.

## Phase 2 — Transactional template catalog

Build the first set (each = html + plaintext + subject):
1. **Invite / onboarding** — the one #65 needs first: "you've been invited to <tenant>", CTA button
   to the accept link, expiry note. **This unblocks #65 Phase 2.**
2. **Password reset** — token link + expiry + "didn't request this" line.
3. **Welcome / confirmed** — post-accept, orient the new user.
4. (Candidate) **Alert / event digest** — a periodic summary from the event stream (#66); likely a
   later card, but design the shell so it can hold a list of events.

## Phase 3 — Preview + test harness

- A **dev preview**: render every template to `.html` files (a `cargo` example / test) and/or a
  gated control route `GET /dev/email/:template` that returns the rendered HTML, so we can eyeball
  in a browser and across a client-preview tool (Litmus/Email-on-Acid-style) before shipping.
- Snapshot the rendered HTML in a test so template changes are reviewable in diffs.
- Verify the plaintext alternative is sane for each.

## Open decisions (grill before building)

1. **Templating engine:** `askama` (compile-time checked, partials, no runtime dep, rustls-clean)
   vs. plain `format!` strings vs. `maud`. Recommend **`askama`** — real reuse for the shared shell.
2. **Ownership split with #65:** does #70 land the whole `email` module (shell + templates + a stub
   sender) and #65 swaps in the real Resend client + invite flow, or does #65 land the module first?
   Recommend **#70 ships shell + templates + dev-log sender; #65 adds real delivery + the invite
   token flow.** Sequence #70 before/with #65.
3. **Where logo art is hosted:** committed static served from `APP_BASE_URL` vs. a CDN/R2 URL.
   Recommend served from the app's `static/brand/` behind `APP_BASE_URL` (simplest; already public).
4. **Dark-mode strategy:** invest in per-client dark overrides, or a single ink palette robust to
   forced inversion? Recommend the robust-single-palette route first (monochrome makes this easy).

## Out of scope (v1)

- The email **sending** primitive/provider integration (that's #65).
- Marketing/newsletter emails, drip campaigns, localization/i18n of templates.
- Per-tenant white-label email branding.

## Verification

- Each template renders to valid, table-based, inline-styled HTML + a plaintext twin; looks correct
  with **images disabled** and on mobile width.
- `cargo check`/tests clean; snapshot tests pass; the dev preview route/example renders all
  templates; spot-check in at least one real client (Gmail) + a preview tool.
- The invite template satisfies #65's needs (accept CTA + expiry) so #65 can send it unchanged.
