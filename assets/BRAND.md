# Olhar AI — Brand / Logo Spec (handoff)

Computer-vision SaaS ("olhar" = gaze, PT). Short form: **OAI**. Monochromatic brand — no hue; branding is carried by form.

## The mark — "Eclipse in orbit"
An eclipsed disc (a gaze emerging from shadow) with a pupil, inside a dotted orbit ring with a small moon. Space + vision.

Geometry (viewBox 0 0 120 120, center 60,60):
- Disc: circle r=31 at (60,60), filled ink.
- Crescent cut: mask circle r=27 at (71,49) (up-right of center) subtracted from the disc.
- Pupil: circle r=6 at (71,49) — same center as the cut.
- Orbit: circle r=50 at (60,60), stroke-width 2, `stroke-dasharray="0.1 7.754"` (40 dots — the period divides the circumference 2π·50 exactly; recompute if you change r), `stroke-linecap="round"`, **opacity 0.3**.
  - At small sizes (<40px): stroke-width 3, dasharray `0.1 9.717` (32 dots), opacity 0.35.
- Moon: circle r=4 (r=5 small) at (95.4, 24.6) — on the orbit at 45°, full ink (no opacity).

## Color
Strictly monochrome, OKLCH greys:
- Ink on dark: `oklch(0.98 0 0)` (#FAFAFA) on `oklch(0.13 0 0)` (#171717) — primary "Noir" use.
- Ink on light: `oklch(0.15 0 0)` (#171717) on `oklch(0.99 0 0)`.
- Never recolor with a hue. State colors (red/amber/green) are for UI status only, never the logo.

## Typography
- Wordmark: **Sora 600 (SemiBold), UPPERCASE**, letter-spacing **0.16em** (0.18em at display sizes). "OLHAR AI" / short "OAI". Google Fonts: https://fonts.google.com/specimen/Sora
- UI font: SF Pro via Apple system stack; mono: SF Mono stack (for technical values, eyebrow labels with ~0.14em tracking).
- Lockup: mark + wordmark side by side, mark ≈1.6× the cap height, gap ≈0.65× cap height.

## Animation (see animated SVGs)
CSS animations inside the SVG (not SMIL — more reliable):
- Orbit ring: rotate 360° / 90s linear, infinite.
- Moon: rotate 360° / 24s linear (rides the ring, passes behind the disc — draw moon before disc).
- Pupil "breathe": scale 1→1.12→1 / 3.4s cubic-bezier(0.4,0,0.6,1).
- Moving eye: translate the crescent-mask group AND pupil group together — glance left (-19,4), down (-8,16), return; 9s loop with holds; cubic-bezier(0.4,0,0.2,1).
- Respect `prefers-reduced-motion` (animation: none).

## Design decisions log
1. Explored 4 monogram directions (orbit/eclipse/constellation/monolith) → chose **Eclipse**.
2. Added an orbit; tried tilted ellipse, full ring + moon, open arc → chose **ring + moon**.
3. Made the ring dotted + low opacity (subtle, reads as stars/orbit) → settled at 30%.
4. Dot spacing tuned so the dash period divides the circumference exactly (uniform dots, no seam).

## Files (assets/)
- `olhar-ai-mark-white.svg` / `-black.svg` — static mark, transparent bg.
- `olhar-ai-app-icon.svg` — mark on #171717 rounded tile (rx 14/120).
- `olhar-ai-lockup-white.svg` / `-black.svg` — OLHAR AI lockup (text uses Sora — load the font or outline it).
- `olhar-ai-oai-white.svg` / `-black.svg` — OAI short lockup.
- `olhar-ai-mark-animated.svg` — orbit + moon + breathing pupil.
- `olhar-ai-mark-animated-eyes.svg` — same + moving-eye glance loop.

Exploration history: `Olhar AI Logo.dc.html` (turns 1–4). Live animation demo: `Animated Mark Demo.dc.html`.
