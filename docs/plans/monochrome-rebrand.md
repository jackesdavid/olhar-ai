# Monochrome Rebrand — Implementation Plan

**Board card:** Black & White Rebrand (ToDo)
**Goal:** Replace the "Emerald Guardian" green identity with a pure black-and-white "Sentinel Noir" theme — impeccable contrast, serious editorial feel — across both the web app and the landing page.

## Design rationale

The new palette is fully achromatic for brand/identity; semantic colors (destructive red, amber "armed" state) survive unchanged because they communicate state, not brand. The only surviving green is on the 3D map floor, where emerald exclusively signals LIVE presence/camera-FOV — a functional distinction that must remain legible.

**Sentinel Noir dark mode surface stack:**
```
Background  oklch(0.07 0 0)   — near-black, no hue
Card        oklch(0.11 0 0)   — slightly lifted
Secondary   oklch(0.16 0 0)   — hover/selected row
Border      oklch(1 0 0 / 8%) — whisper border
Primary     oklch(0.97 0 0)   — white CTA, logo, ring
```
**Contrast ratios:** background ↔ foreground ~17:1 (exceeds AAA). Primary white on background ~14:1.

---

## Phase 1 — CSS variable foundations (sentinel-app/src/app.css + landing/src/app.css)

This phase is ~80% of the visible change — all shadcn components inherit CSS variables.

### sentinel-app/src/app.css

Rename theme comment to "Sentinel Noir".

**`:root` (light fallback):**
- `--background`: `oklch(0.99 0 0)` — pure white
- `--foreground`: `oklch(0.07 0 0)` — near-black
- `--card`: `oklch(1 0 0)` — white
- `--card-foreground`: `oklch(0.07 0 0)`
- `--popover`: `oklch(1 0 0)` / `oklch(0.07 0 0)`
- `--primary`: `oklch(0.07 0 0)` — black primary in light mode
- `--primary-foreground`: `oklch(0.99 0 0)` — white text on black
- `--secondary`: `oklch(0.95 0 0)` / `oklch(0.2 0 0)`
- `--muted`: `oklch(0.95 0 0)` / `oklch(0.55 0 0)`
- `--accent`: `oklch(0.92 0 0)` / `oklch(0.2 0 0)`
- `--border`: `oklch(0.88 0 0)` / `oklch(0.07 0 0)`
- `--input`: `oklch(0.88 0 0)` / `oklch(0.07 0 0)`
- `--ring`: `oklch(0.07 0 0)`
- Sidebar mirrors `--background` / `--foreground` neutral values
- Remove all `162` hue tokens throughout

**`.dark` block (operative):**
- `--background`: `oklch(0.07 0 0)`
- `--foreground`: `oklch(0.97 0 0)`
- `--card`: `oklch(0.11 0 0)`
- `--card-foreground`: `oklch(0.97 0 0)`
- `--popover`: `oklch(0.13 0 0)` / `oklch(0.97 0 0)`
- `--primary`: `oklch(0.97 0 0)` — white
- `--primary-foreground`: `oklch(0.07 0 0)` — black
- `--secondary`: `oklch(0.16 0 0)` / `oklch(0.97 0 0)`
- `--muted`: `oklch(0.16 0 0)` / `oklch(0.6 0 0)`
- `--accent`: `oklch(0.2 0 0)` / `oklch(0.97 0 0)`
- `--destructive`: `oklch(0.7 0.19 22)` — keep red, semantic
- `--border`: `oklch(1 0 0 / 8%)`
- `--input`: `oklch(1 0 0 / 10%)`
- `--ring`: `oklch(0.97 0 0)`
- `--sidebar`: `oklch(0.09 0 0)`
- Sidebar accent/border/ring mirror the neutral `--accent` / `--border` / `--ring`

**Utility classes in `@layer utilities`:**
- `.interactive:hover` border → `oklch(1 0 0 / 25%)`; shadow unchanged (black)
- `.glow` → `0 0 0 1px oklch(1 0 0 / 18%), 0 0 26px -6px oklch(1 0 0 / 25%)`
- `.status-live` animation → pulses with `oklch(1 0 0 / 0.5)` (white glow)
- `body` radial gradient → remove emerald glow or replace with `oklch(1 0 0 / 0.03)` top shimmer
- `::selection` → `oklch(1 0 0 / 0.2)` (white selection)
- `.text-grad` → `linear-gradient(110deg, oklch(0.97 0 0), oklch(0.75 0 0) 60%, oklch(0.55 0 0))`
- `.node-running` / `.node-run-glow` — `--march: var(--primary)` already uses the token; no code change
- `.list-running` `--march` → inherits `var(--primary)` (now white) — no code change
- `.list-idle` amber `oklch(0.77 0.16 78)` — KEEP. Amber is semantic ("armed, not firing").
- `@keyframes status-pulse` → update box-shadow to `oklch(1 0 0 / 0.5)` / `oklch(1 0 0 / 0)`
- `@keyframes node-glow` → no change needed (uses SVG attribute color set at call site)
- `@keyframes ping-ring` → no change needed (border colour inherits `border-primary`)

### landing/src/app.css

Same variable replacements (this file is a subset). Also update:
- `body` background gradient → remove emerald glow or faint white
- `::selection` → `oklch(1 0 0 / 0.2)`
- `.text-grad` → white-to-gray gradient
- `.status-live` / `.ping-ring` keyframes → white

---

## Phase 2 — 3D map palette (sentinel-app/src/lib/map/palette.ts)

The map 3D layer is mostly monochrome already. Only the LIVE accent is emerald. Decision: **keep map accent emerald** — it's a *functional* affordance (live state on a dark drafting floor), distinct from brand identity. This is the only green in the product after the rebrand.

No file change needed in Phase 2. Revisit if the user wants a white-glow presence pulse instead.

---

## Phase 3 — Hardcoded Three.js / sensor-field colors (Svelte files)

Files and changes:

| File | What to change |
|------|----------------|
| `lib/components/sensor-field.svelte` | `const RGB = "52,211,153"` → `"180,180,180"` (neutral dot lattice) |
| `lib/workflows/portColors.ts` | `num: "#34d399"` → `"#e5e5e5"` (white-ish port nub) |
| `lib/three/map/DrawPreview.svelte` | All `#34d399` / `#6ee7b7` → `#e5e5e5` (draw rubber-band stays white) |
| `lib/three/map/CameraMarker.svelte` | `color="#34d399" emissive="#10b981"` → `color="#e5e5e5" emissive="#ffffff"` |
| `lib/three/map/PresencePulse.svelte` | default `color="#34d399"` → `"#e5e5e5"` |
| `lib/map/categories.ts` | `known_person: "#34d399"` → `"#e5e5e5"` (keep other categories if they differ) |

---

## Phase 4 — Tailwind hard-coded class names in route pages

Files using `text-emerald-*`, `bg-emerald-*`, `border-emerald-*`, `drop-shadow-[..162..]`:

| File | Change |
|------|--------|
| `routes/+layout.svelte:287` | `drop-shadow-[0_0_10px_oklch(0.7_0.16_162/0.45)]` → `drop-shadow-[0_0_10px_oklch(1_0_0/0.3)]` |
| `routes/recordings/+page.svelte:63` | `text-emerald-400` → `text-primary` |
| `routes/admin/providers/+page.svelte:71` | `border-emerald-500/30 bg-emerald-500/15 text-emerald-400` → `border-border bg-muted text-foreground` |
| `routes/workflows/+page.svelte:1794` | `flood-color="#10b981"` → `flood-color="#e5e5e5"` |
| `routes/logs/+page.svelte:183` | `presence: "bg-emerald-500/15 text-emerald-400"` → `"bg-muted text-foreground"` |
| `routes/logs/+page.svelte:191` | `workflow: "#10b981"` → `"#e5e5e5"` |

---

## Phase 5 — Landing page inline styles (landing/src/routes/+page.svelte)

All `oklch(0.7 0.16 162/...)` inline references → `oklch(1 0 0 / ...)` (white glow):
- Hero logo drop shadow (line 75, 97)
- CTA button shadow (lines 85, 116)
- `ping-ring` border color (lines 95–96 — inherits `border-primary`, no code change if CSS vars are right)
- Camera mockup border + shadow (lines 143–150)
- Status live dot (inherits `bg-primary` — auto from CSS vars)

---

## Open decisions

1. **Map accent color** — keep emerald for live/presence or go all-white? Emerald is more legible as "live" on the dark floor (green = on, white = neutral). Default plan: keep emerald on the map.
2. **Sidebar radius** — currently `0.625rem`. Tighten to `0.375rem` for a sharper, more serious feel?
3. **Sensor field dots** — neutral grey keeps the animated background; removing it entirely is also valid for a starker look.

## Out of scope (v1)

- Typography changes (font family / weight)
- Logo mark shape changes (only color)
- Component-level redesigns (layout, spacing, iconography)
- Dark/light mode toggle behavior change
