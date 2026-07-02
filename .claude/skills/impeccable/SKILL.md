---
name: impeccable
description: Design-excellence skill for the Sentinel web app — enforce a polished, consistent, production-grade UI/UX built on the "dot-field" visual identity (the hexagonal sensor-field logo) and the monochrome ink theme (Clean White / Sentinel Noir). Invoke when building or polishing any screen, component, animation, or visual.
---

# Impeccable — Sentinel design excellence

Make every surface look and feel **intentional, alive, and consistent**. The bar is
"a product someone would pay for." Apply this whenever touching UI.

## The visual identity — a dot-field sensor

The Sentinel logo is a **hexagon of circles**: dots largest/brightest at the center,
shrinking and dimming toward the edge — a field of awareness condensing to a focal
point. That is the whole system's DNA. Everything should echo it:

1. **The dot/node is the atom.** Circles, dot grids, node graphs, status dots,
   halftone textures. Prefer dots over bars/blocks for accents, indicators, loaders.
2. **Radial falloff = hierarchy.** Bright/large/dense at the focal point, dim/small/
   sparse at the periphery. Use for emphasis, glows, backgrounds, loading.
3. **Hexagonal / 6-fold geometry & generous negative space.** Centered, calm
   compositions. Restraint. Let the focal element breathe.
4. **Signal pulses.** The "ping" — concentric rings radiating from a point — is the
   signature motion (a sensor detecting). Use for live/online state, loading, and
   confirmation, not gratuitously.

## Theme (monochrome ink — see styles/sentinel-tokens.css, imported by app.css)

Branding is carried by **form, not colour** — the dot-field mark, radial falloff,
generous negative space, and one near-black↔white ink. Two operative themes toggle on
the `.dark` class (default is light):
- **Clean White** (light, default): soft-gray page, pure-white cards lift off it,
  near-black ink.
- **Sentinel Noir** (dark): near-black canvas ramp, off-white ink.

- `--primary` is near-black (light) / white (dark) — active/online/armed, links, and
  primary actions. **No hue on surfaces or the primary action.** Never reintroduce the
  old emerald accent or the stock zinc near-white.
- Semantic colour survives ONLY because it communicates STATE, not brand:
  danger=red (`--destructive`), warn/armed=amber. There is no brand green — "live"
  reads through the dot-field pulse (`.status-live`), never a colour swatch.
- Reuse the utilities: `.interactive` (hover lift + inked border), `.glow`, `.press`,
  `.status-live` (the radial pulse), `.text-grad` (inked hero headings). Effect tokens
  (`--lift-*`, `--glow-*`, `--pulse-color`, `--shimmer`, `--grad-*`) live in the tokens
  file so they read on either canvas — add effects there, never one-off per-file CSS.
- The shared `SentinelMark` component is the canonical dot-hex; use it (via `Loader` /
  `EmptyState`) for the brand, loaders, and empty states — never a generic spinner or
  placeholder.
- The workflow node-editor and 3D map are the deliberate exceptions: colour is their
  focal identity (fixed categorical palettes in `portColors.ts` / map `palette.ts`).
  Don't monochrome those; don't spread their colour elsewhere.

## Craft checklist (every screen)

- **States:** loading (dot-field, not a bare spinner), empty (the mark + one helpful
  line + a primary action), error (clear, recoverable), and the happy path.
- **Motion:** entrance (fade/slide via `animate-in`), hover/active feedback on every
  interactive element, smooth transitions (150–250ms, ease-out). Respect
  `prefers-reduced-motion`. Motion must mean something — feedback, state, or arrival.
- **Hierarchy:** one clear focal point per view; size/weight/color earn attention in
  that order. Consistent spacing rhythm (4px base). Canonical type scale (match, don't
  reinvent): page title = `text-2xl font-semibold tracking-tight`; section eyebrow =
  `text-xs font-semibold uppercase tracking-wide text-muted-foreground`; body/captions
  ride on `text-sm`/`text-xs` + `text-muted-foreground`. `tabular-nums` on aligned or
  ticking numbers (stat tiles, table columns, counters) — NOT on numbers inside running
  prose. Line-breaking is handled globally in app.css (`text-wrap: balance` on headings,
  `pretty` on `<p>`); don't re-solve per screen.
- **Depth:** subtle — borders + soft shadows + the dot-field ambient bg, not heavy
  drop shadows. The `.glow` / brand glow only on focus/active/live.
- **Consistency:** reach for the shared components before hand-rolling — `PageHeader`
  (title + nav icon-chip + description + `actions` snippet; every page header uses it),
  `IconChip` (the canonical `bg-primary/10 text-primary ring-1 ring-inset ring-primary/20`
  chip), `Loader`/`Busy` (dot-field loaders — never a bare spinner), `EmptyState` (mark +
  line + action). Status pills = `border-primary/25 bg-primary/10 text-primary`;
  rows/cards/tiles use `.interactive`.
- **No dead ends:** every screen tells you what to do next. No `window.prompt/confirm`
  (no-ops in the Tauri WebView) — use dialogs.

## Process

When asked to polish/build UI: (1) name the focal point and the states; (2) reach for
the dot-field motif + existing utilities/`SentinelMark` before inventing; (3) build it;
(4) `npm run build`; (5) keep it consistent with sibling screens. Tune, don't settle.
