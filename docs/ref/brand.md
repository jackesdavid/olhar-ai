# Brand — Sentinel identity reference

The visual identity: the **dot-field mark** + the **Sentinel wordmark**. Monochrome ink; branding
carried by *form*, not colour. See also the `impeccable` skill and `[[design-system]]`.

## The mark — dot-field

A hexagonal field of circles with radial size + opacity falloff: dense/bright at the centre,
sparse/dim at the edge — a sensor's field of awareness condensing to a focal point. Optionally
"pings" (a slow radial ripple) for live/loading states.

- **Component:** `sentinel-app/src/lib/components/sentinel-mark.svelte` — procedural SVG,
  `currentColor`, props `size`, `animated`, `rings`, `speed`.
- **Canonical static mark = `rings=4`** (the approved "#01" refinement). The animated
  loader/splash variant uses the component default (`rings=5`, `animated`).
- **Master file:** `sentinel-app/static/brand/mark.svg` (rings=4, `fill="currentColor"`).

## The wordmark — Sora, sentence-case

"Sentinel" set in **Sora 600, sentence-case**, tracking `-0.02em`. Chosen for the "calm assistant,
plain terms" brand (calm geometric; the UPPERCASE-tracked cut is the formal/institutional alternate,
not the default). Sora is **self-hosted** via `@fontsource/sora/600.css` (imported once in
`src/routes/+layout.svelte`) and exposed as the token **`--font-brand`** (`"Sora", var(--font-sans)`)
in `styles/sentinel-tokens.css`. **Scope: the logo only** — it does NOT change the app's UI body
font (`--font-sans`, still SF Pro/Inter stack).

## The lockup — `SentinelLogo`

`sentinel-app/src/lib/components/sentinel-logo.svelte` — the mark + wordmark, cap-height matched.

| Prop | Default | Use |
|---|---|---|
| `size` | 30 | mark height in px; wordmark derives from it (`×0.66` horizontal, `×0.5` stacked) |
| `wordmark` | true | false = mark only (e.g. collapsed sidebar) |
| `stacked` | false | wordmark under the mark (square/hero contexts) |
| `animated` | false | a logo shouldn't ping constantly; on only where deliberate |
| `subtitle` | "" | tagline under the wordmark (e.g. "Monitoring & Automation") |

Clear-space + cap-height ratios live in the component (gap `×0.36` / `×0.24`). Change them there,
once — never hand-roll a lockup inline.

**In use:**
- Sidebar header (`+layout.svelte`): `<SentinelLogo size={30} wordmark={!navCollapsed} subtitle="Monitoring & Automation" />`.
- Login card (`login/+page.svelte`): `<SentinelLogo stacked size={52} />`.
- Splash (`splash.svelte`): the animated **mark only** (a valid mark-only lockup).

## Do / don't

- **Don't** recolour the mark or wordmark with a brand hue — monochrome ink only; only *state*
  colour (danger/warn) survives, and the logo carries no state.
- **Don't** set the wordmark in any font but Sora, or change its case away from sentence-case
  (except the sanctioned UPPERCASE alternate).
- **Don't** hand-place mark+text; use `SentinelLogo`.
- **Do** use the mark alone below ~24px legibility (favicon uses a dedicated small treatment — #69).

## Build status

- **Done (#68):** mark refined (rings=4) + master `mark.svg`; Sora self-hosted + `--font-brand`;
  `SentinelLogo` lockup; wired into sidebar + login; `docs/ref/brand.md`. `npm run build` +
  `npm run check` clean (no new errors); login lockup verified in-app (light + Noir).
- **Pending:** favicons / app-icons / PWA / OG images from this mark (**#69**); email header art
  (**#70**). Raster/outlined-text lockup exports (for email/OG, where Sora isn't available) are
  produced there via headless render of `SentinelLogo`.
- **Backlog:** landing site (`landing/`, `sentinel-landing/`) asset swap; a fuller brand book.
