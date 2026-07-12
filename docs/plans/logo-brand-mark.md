# Logo & Brand-Mark System — Implementation Plan

**Use case:** inline — "we still don't have a great logo"; expand the visual identity.
**Board card:** Logo & brand-mark system (ToDo, #68)
**Goal:** Land a **great, distinctive logo** and turn it into a real **logo system** — mark +
wordmark lockup, light/dark variants, clear-space & min-size rules, and static exports at every
size/format the product needs (favicons #69, email #70, OG images, app chrome). Direction chosen:
**explore fresh concepts first**, then commit and systematize.

## Decisions locked (2026-07-11)

- **Mark:** the **guarded perimeter, #05a** — the dot-field + solid core inside a rounded
  hexagonal boundary ("data in your control"). Outcome (A): dot-field DNA preserved, elevated.
  Explored via the installed `svg-design` skill (6 concepts → #05 → tuned 05a). Rendered by
  `SentinelMark boundary`; the bare pulsing field stays the "active"/loader expression.
- **Wordmark:** **Sora 600, sentence-case** ("Sentinel"), tracking `-0.02em`. UPPERCASE-tracked is
  the sanctioned formal alternate. Scoped to the logo via `--font-brand`; UI body font unchanged.
- **Status:** Phase 1 (explore) + Phase 2 (choose) done. Phase 3 (productionize) wired for the app
  + favicons + OG (#69 regenerated from 05a); see `docs/ref/brand.md`. Email rasters → #70.

## Current state (grounding)

- The identity today is the **dot-field mark**: `sentinel-app/src/lib/components/sentinel-mark.svelte`
  (also `landing/src/lib/components/sentinel-mark.svelte`) — a *procedural* SVG, a pointy-top hex
  lattice of circles with radial size+opacity falloff and a slow "ping" animation. It's
  `currentColor` (monochrome), rendered inline; **there is no static exported asset, no wordmark,
  no lockup.**
- It IS the established identity per `[[design-system]]`, `[[monochrome-rebrand]]`, and the
  `impeccable` skill (dot-field = the whole system's DNA; branding by form, not colour).
- Leftover template cruft in `sentinel-app/static/`: `svelte.svg`, `tauri.svg`, `vite.svg`,
  `logo-inspiration.gif` — to be removed in #69.

## Identity tension (read before exploring)

The design system currently *defines* the brand as the dot-field mark. Exploring fresh concepts
means one of two outcomes, and we must be explicit about which:
- **(A) A refined dot-field wins** — the DNA is preserved; we upgrade craft + add a wordmark. No
  system-doc churn.
- **(B) A different concept wins** — then `[[design-system]]`, `[[monochrome-rebrand]]`, the
  `impeccable` skill's "dot-field" language, and the `SentinelMark` component all change together.
  Don't half-migrate: if we replace the mark, update every reference + the memory in one sweep.

## Phase 1 — Explore concept directions (diverge)

Produce a **comparison set** of candidate marks, then pick. Tools: the **`svgmaker` MCP**
(`svgmaker_generate` / `svgmaker_edit` for vector concepts), the **`frontend-design`** skill (craft
+ distinctiveness, avoid generic AI aesthetics), and **`impeccable`** (keep it coherent with the
ink theme + dot-field heritage as a baseline candidate).

Candidate directions to generate (≥4, monochrome-first so colour never carries the brand):
1. **Refined dot-field** — the current DNA, higher craft (perfected falloff, optical centering, a
   crisp static form distinct from the animated component).
2. **Hex monogram** — an "S" / sentinel glyph inscribed in the 6-fold geometry.
3. **Sensor aperture / concentric rings** — the "ping" frozen into a mark (a lens/eye/aperture).
4. **Shield ⊕ dot-field hybrid** — security connotation fused with the sensor field.

Deliverable: a **side-by-side comparison** (an Artifact or a static gallery page) showing each at
hero, small (24px), and monochrome on both Clean White + Sentinel Noir — so we judge them as a
system, not as pretty one-offs. Decide the winner here.

## Phase 2 — Design the final mark + wordmark lockup (converge)

- Refine the chosen direction to a **final, hand-tuned SVG** (optical alignment, consistent stroke
  logic, works at 16px through hero). Ship crisp static + an animated variant if the dot-field wins
  (reuse the `.status-live` ping language).
- **Wordmark:** typeset "Sentinel" — pick the face (evaluate the app's existing UI font first for
  coherence), tune tracking/case. Monochrome ink.
- **Lockup system:** primary (mark + wordmark, horizontal), mark-only (favicon/app-icon), wordmark-
  only, plus **clear-space** and **min-size** rules. **Light + dark** variants (near-black ink on
  Clean White; off-white on Sentinel Noir).
- Document the whole thing in a new **`docs/ref/brand.md`** (identity, do/don't, the lockups, export
  matrix) and add a pointer in `CLAUDE.md`'s index.

## Phase 3 — Productionize + wire in

- **Master SVGs** committed under `sentinel-app/static/brand/` (mark, wordmark, lockup; light/dark).
- **Upgrade `SentinelMark`**: keep the ergonomic Svelte component API (`size`, `animated`), but
  align its output to the final mark; update the `landing/` copy in lockstep (single source or
  mirrored).
- **Static raster exports** for surfaces that can't use the component: email header (#70), OG image
  (#69) — a small export step (SVG → PNG at needed sizes; note the tool used, no runtime dep).
- Feeds **#69** (favicons/app-icons — needs the final mark) and **#70** (email header art).
- If outcome (B): sweep `[[design-system]]`, `[[monochrome-rebrand]]`, the `impeccable` skill doc,
  and the `[[design-system]]` memory so the identity language matches the new mark.

## Open decisions (grill before building)

1. **Keep the dot-field, or replace it?** Phase 1's whole point — but bias: the dot-field is
   genuinely strong and already load-bearing across the app + map markers + loaders. Replacing it
   is a large blast radius. Recommend the refined dot-field be a *serious* candidate, not a
   formality.
2. **Wordmark typeface:** reuse the UI font (coherence, zero new asset) vs. a distinctive display
   face (more ownable, another dependency). Recommend evaluate UI font first.
3. **Animated logo scope:** where does the "ping" live (loaders/splash/live-state only) vs. a
   static logo everywhere else? Keep motion meaningful, not decorative.
4. **Colour ever?** Identity is monochrome ink by rule. Confirm we hold that for the logo too (only
   state-colour survives, and the logo carries no state). Recommend yes, stay monochrome.

## Out of scope (v1)

- Full brand book beyond `docs/ref/brand.md` (illustration style, photography, motion system).
- Rebranding the landing marketing site's layout (only the mark/wordmark asset swaps).
- Animated OG/social video.

## Verification

- Judge candidates as a *set* at hero + 24px on both themes before committing.
- After wire-in: `npm run build` clean; the mark reads at favicon size; app + landing + map markers
  + `Loader`/`EmptyState` all still render the mark correctly; `docs/ref/brand.md` exported matrix
  matches the committed files.
