# Brand — Filma AI identity reference

The visual identity: the **eclipse-eye mark** + the **FILMA AI wordmark**. Monochrome ink; branding
carried by *form*, not colour. The authoritative spec is **`assets/BRAND.md`** — this file is the
codebase's map onto it. See also the `impeccable` skill and `[[design-system]]`.

*"olhar"* is Portuguese for *gaze*. The product watches; the mark is an eye.

## The mark — "Eclipse in orbit"

An **eclipsed disc** (a gaze emerging from shadow) with a **pupil**, inside a **dotted orbit ring**
with a small **moon**. Space + vision. The ring is dotted at low opacity — it *evolves* the old
dot-field language rather than discarding it; the monochrome ink is unchanged.

Geometry (viewBox `0 0 120 120`, centre 60,60), verbatim from `assets/BRAND.md`:
- Disc `r=31`, ink-filled. Crescent cut by masking a circle `r=27` at (71,49). Pupil `r=6`, same
  centre. Orbit `r=50`, dotted (`stroke-dasharray="0.1 7.754"`, 40 dots), opacity 0.3. Moon `r=4` at
  (95.4, 24.6), on the ring at 45°.
- **Small variant (<40px)**: stroke 3, dasharray `0.1 9.717` (32 dots), opacity 0.35, moon `r=5` —
  or the dots mush. Auto-selected by `size`.
- **Minimal (<24px, e.g. a 16px favicon or an inline `busy` spinner)**: drop the orbit + moon, crop
  to the eye. The eye carries the identity when the ring can't survive.

One procedural component, `sentinel-app/src/lib/components/olhar-mark.svelte` (the file keeps its
`sentinel-*` path — the rebrand renames what users see, not the codebase). `currentColor`, so one
mark themes light/dark; per-instance mask ids so two marks on a page don't collide. Props:
`size`/`animated`/`blink`/`tile`/`pace`/`label`. `landing/` has a byte-identical copy (separate build,
no shared lib — intent to unify noted in card #100).

### Motion (BRAND.md § Animation)
- Orbit ring rotates 360° / 90s. Moon 360° / 24s, drawn **before** the disc so it passes *behind* the
  eye. Pupil "breathe" scale 1→1.12→1 / 3.4s.
- **The eye behaves like an eye**, not a planet: it holds its place, gazes in **saccades** (quick
  shifts, long fixations — a gliding pupil reads as drunk), and **blinks** (ours, not in BRAND.md, on
  a `blink` prop; the lid is the eye-opening circle flattening so the crescent fills back in).
- `prefers-reduced-motion` → none; the static state is the supplied art exactly.

## The wordmark — Sora 600, UPPERCASE

**"FILMA AI"** set in **Sora 600, UPPERCASE**, letter-spacing **0.16em** (0.18em at display sizes).
The short submark for tight spaces is **"OAI"** — a sub-mark of the same brand, not a separate one.
The written product name is **"Filma AI"** (two words); the wordmark styling is the uppercase lockup.

Sora is self-hosted via `@fontsource/sora/600.css` (imported once in `src/routes/+layout.svelte`) and
exposed as **`--font-brand`**. **Scope: the logo only** — it does not change the app's UI body font
(`--font-sans`, SF Pro / Inter stack).

## The lockup — `SentinelLogo`

`sentinel-app/src/lib/components/sentinel-logo.svelte` composes the mark + the FILMA AI wordmark.
Cap-height ratios from BRAND.md: mark ≈ 1.6× cap height, gap ≈ 0.65× cap. The component keeps its
`SentinelLogo` name (codebase, not user-facing).

| Prop | Default | Use |
|---|---|---|
| `size` | 30 | mark height in px; wordmark cap-height-matched to it |
| `wordmark` | true | false = mark only (e.g. collapsed sidebar) |
| `stacked` | false | wordmark under the mark (square/hero contexts) |
| `animated` | false | a logo shouldn't ping constantly; on only where deliberate |

## Colour

Strictly monochrome, OKLCH greys — **never a hue**. Ink on dark `#FAFAFA` on `#171717` (primary
"Noir"); ink on light `#171717` on `#FAFAFA`. State colours (red/amber/green) are for UI status only,
never the logo.

## Assets

- Masters: `assets/olhar-ai-*.svg` (mark, app-icon, FILMA AI + OAI lockups, animated) + `assets/BRAND.md`.
- Rendered set: `sentinel-app/static/` (favicon `.ico`/`.svg`/16/32, apple-touch, icon-192/512 +
  maskable). Re-cut from the mark by `sentinel-app/scripts/build-icons.mjs` — re-run on any change,
  don't hand-edit.
- **Still pending (#76):** `og.png` — needs the lockup rendered with Sora, which ships via npm and
  isn't a system font, so the local `sharp` pipeline can't render it.
