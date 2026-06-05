---
name: impeccable
description: Design-excellence skill for the Sentinel web app — enforce a polished, consistent, production-grade UI/UX built on the "dot-field" visual identity (the hexagonal sensor-field logo) and the Emerald Guardian theme. Invoke when building or polishing any screen, component, animation, or visual.
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
   compositions on near-black. Restraint. Let the focal element breathe.
4. **Signal pulses.** The "ping" — concentric rings radiating from a point — is the
   signature motion (a sensor detecting). Use for live/online state, loading, and
   confirmation, not gratuitously.

## Theme (Emerald Guardian — see app/src/app.css)

- Emerald `--primary` for active/online/armed, links, primary actions. Never the old
  zinc near-white. Surfaces are a near-black green-neutral ramp; emerald focus ring.
- Semantic: online=emerald, warn=amber, danger=red, info=sky.
- Reuse the utilities: `.interactive` (hover lift + emerald border), `.glow`,
  `.press`, `.status-live` (the radial pulse). Add new ones rather than one-off CSS.
- The shared `SentinelMark` component is the canonical dot-hex; use it for the brand,
  loaders, and empty states (never a generic spinner or placeholder).

## Craft checklist (every screen)

- **States:** loading (dot-field, not a bare spinner), empty (the mark + one helpful
  line + a primary action), error (clear, recoverable), and the happy path.
- **Motion:** entrance (fade/slide via `animate-in`), hover/active feedback on every
  interactive element, smooth transitions (150–250ms, ease-out). Respect
  `prefers-reduced-motion`. Motion must mean something — feedback, state, or arrival.
- **Hierarchy:** one clear focal point per view; size/weight/color earn attention in
  that order. Tabular-nums for numbers. Consistent spacing rhythm (4px base).
- **Depth:** subtle — borders + soft shadows + the dot-field ambient bg, not heavy
  drop shadows. Emerald glow only on focus/active/live.
- **Consistency:** icon chips = `bg-primary/10 text-primary ring-1 ring-inset
  ring-primary/20`; status pills = `border-primary/25 bg-primary/10 text-primary`;
  rows/cards/tiles use `.interactive`.
- **No dead ends:** every screen tells you what to do next. No `window.prompt/confirm`
  (no-ops in the Tauri WebView) — use dialogs.

## Process

When asked to polish/build UI: (1) name the focal point and the states; (2) reach for
the dot-field motif + existing utilities/`SentinelMark` before inventing; (3) build it;
(4) `npm run build`; (5) keep it consistent with sibling screens. Tune, don't settle.
