# Favicons / App-Icons / PWA / OG Images — Implementation Plan

**Use case:** inline — expand the visual identity; "favicons don't exist yet."
**Board card:** Favicons / app-icons / PWA / OG images (ToDo, #69)
**Goal:** A complete, correct icon + social-preview set derived from the final logo — every
favicon size/format, apple-touch, maskable PWA icons + web manifest, `theme-color`, and Open
Graph / Twitter share images with meta — replacing today's lone stopgap PNG.

**Blocked on #68** (needs the final mark/lockup). Uses `impeccable` + `frontend-design` for the OG
image composition.

## Status — done (2026-07-11)

Generated from the locked dot-field mark (Noir tile + off-white dots): `favicon.svg`, `favicon.ico`
(16/32/48), `favicon-16/32.png`, `apple-touch-icon.png` (180), `icon-192/512.png` + maskable
variants, `og.png` (1200×630 lockup, Sora wordmark). `manifest.webmanifest` + full `app.html` head
(icons, apple-touch, manifest, light/dark `theme-color`, OG/Twitter meta → `app.thesentinels.online`).
Cruft removed (svelte/tauri/vite svg, logo-inspiration.gif, old favicon.png). Icons via `sharp`,
`.ico` via `png-to-ico`, OG via headless Chrome (needs Sora). `npm run build` clean. Ships with #68
as one identity drop.

## Current state (grounding)

- `sentinel-app/src/app.html` links **one** icon: `<link rel="icon" href="…/favicon.png" />`. No
  apple-touch, no `.ico`, no sized PNGs, no `manifest`, no `theme-color`, no OG/Twitter meta.
- `sentinel-app/static/` has `favicon.png` (1.5KB stopgap) + **cruft to delete**: `svelte.svg`,
  `tauri.svg`, `vite.svg`, `logo-inspiration.gif`.
- `<title>Sentinel</title>` is set; theme is pre-painted via the inline script (keep it).
- The landing site (`landing/`, `sentinel-landing/`) needs the same treatment.

## Phase 1 — Favicon + app-icon set (from #68's mark)

Generate from the final mark-only lockup (mind **min-size legibility** — the dot-field must read at
16px; if the fine dots vanish, use a simplified small-size variant per #68's rules):

- `favicon.svg` (modern, scalable, `prefers-color-scheme`-aware if we want auto light/dark).
- `favicon.ico` (multi-res 16/32/48 for legacy).
- `favicon-16.png`, `favicon-32.png`.
- `apple-touch-icon.png` (180×180, opaque background — iOS has no transparency).
- PWA: `icon-192.png`, `icon-512.png`, plus **maskable** variants (safe-zone padded).
- All committed under `sentinel-app/static/` (and mirrored to the landing site).

## Phase 2 — Web manifest + `app.html` head

- New `sentinel-app/static/manifest.webmanifest`: `name` "Sentinel", `short_name`, `description`,
  `start_url`, `display: standalone`, `background_color` + `theme_color` (the ink canvas — note
  light default), `icons` (192/512 + maskable with `"purpose": "maskable"`).
- Rewrite the `app.html` `<head>` icon block: `icon` (svg + png fallback), `apple-touch-icon`,
  `mask-icon`, `manifest` link, and **`theme-color` metas** (with `media` for light/dark so the
  browser chrome matches the active theme).
- **Delete the cruft** SVGs + `logo-inspiration.gif` from `static/`.

## Phase 3 — Open Graph / social share images

- Design a **1200×630 OG image** (`impeccable` + `frontend-design`): the lockup on the ink canvas
  with generous negative space + dot-field ambient — one for light-canvas default (+ optional
  Noir). Committed as a static PNG under `static/brand/` (from #68's export step).
- Add OG/Twitter meta in the root `+layout.svelte` head (or `app.html`): `og:title`,
  `og:description`, `og:image`, `og:url`, `og:type`, `twitter:card=summary_large_image`,
  `twitter:image`. Keep defaults tenant-agnostic (the product, not a tenant).
- Optional later: per-route OG (e.g. a shared clip) — out of scope v1.

## Open decisions (grill before building)

1. **Auto light/dark favicon** via `favicon.svg` + `prefers-color-scheme`, or a single monochrome
   mark that reads on any tab colour? Recommend a single robust monochrome SVG + `.ico`/png
   fallbacks; revisit auto-switch only if it looks weak on one theme.
2. **Small-size legibility:** does the dot-field survive at 16px, or do we need #68's simplified
   small variant? Decide against the real generated 16px, not a mockup.
3. **Generation tooling:** hand-export from the master SVG vs. a generator (realfavicongenerator-
   style). Recommend a documented one-shot export from the #68 master so it's reproducible; note
   the exact commands/tool in `docs/ref/brand.md`.
4. **Manifest scope:** are we actually shipping an installable PWA (service worker), or just a
   correct manifest for icons/theming? Recommend manifest-for-icons now; full PWA/offline is a
   separate effort.

## Out of scope (v1)

- Service worker / offline PWA behaviour.
- Per-route dynamic OG image generation.
- Native app store icon sets (this is web-only — see `[[web-only-cloud]]`).

## Verification

- Lighthouse/PWA installability check passes for icons + manifest; no console 404s for any icon.
- Tab favicon crisp on light and dark browser chrome; iOS "Add to Home Screen" shows the apple-touch
  icon; Android install shows the maskable icon without clipping the mark.
- OG image renders correctly in a link-preview validator (share debugger); `theme-color` matches the
  active theme.
