# Dark Palette — Elevation Ramp (Unreal-style) — Implementation Plan

**Board card:** #72 Dark palette: deeper elevation ramp (Unreal-style) (ToDo)
**Design standard:** the `/impeccable` skill — monochrome ink, form over colour, dot-field identity.
**Goal:** Replace the flat 6-level Sentinel Noir ramp with a properly differentiated, Unreal-Engine-flavoured
elevation scale so nested surfaces read at a glance, and fix the depth mechanics that silently don't work on a
near-black canvas.

---

## The insight that drives this plan

"Make it more like Unreal" does **not** mean "make it darker". Measured in OKLCH lightness, Unreal Engine 5's
Slate dark theme is:

| UE Slate role | Hex | OKLCH L |
|---|---|---|
| Recessed (inputs, wells) | `#0A0A0A` | **0.145** |
| Header / Title | `#0F0F0F` | **0.168** |
| Background (canvas) | `#151515` | **0.196** |
| Panel | `#1E1E1E` | **0.235** |
| Panel raised | `#242424` | **0.261** |
| Dropdown / Foreground | `#383838` | **0.341** |
| Hover | `#575757` | **0.457** |

Sentinel Noir today: `--well` 0.04 · `--background` 0.07 · `--sidebar` 0.10 · `--card` 0.13 · `--secondary` 0.17
· `--muted` 0.17 · `--accent` 0.22 · `--popover` 0.28.

Three conclusions:

1. **Our floor is far too low.** UE's *darkest* surface (0.145) is brighter than our *card* (0.13). We're painting
   the whole app in the 0.04–0.13 band, where human value discrimination is at its worst — everything mushes
   into one black plane.
2. **Because the canvas is near-black, our depth effects are no-ops.** `--lift-shadow` is
   `0 10px 30px -14px oklch(0 0 0 / 80%)` — a *black* shadow on a 0.07 canvas. It is literally invisible. Same for
   the dialog overlay (`bg-black/10` on 0.07 — the modal barely separates from the page). We're paying for depth
   we never render. UE solves this by living higher up the ramp and letting **value steps + brightened borders**
   carry depth, not shadows.
3. **UE's ramp is tighter but has more usable stops** (7 real levels inside a 0.31 range). Ours spans 0.24 but
   has only **6 real levels — `--muted` and `--secondary` are the same value (0.17)** — and the biggest gap in the
   whole stack (`--card` 0.13 → `--popover` 0.28) has *nothing* in it. The codebase already admits this:
   `sentinel-app/src/routes/workflows/[[id]]/+page.svelte:4230` fakes the missing stop —
   ```css
   /* --card alone is too dark; mixing with --popover lifts it ~2 tonal steps. */
   :global(.node-body) { fill: color-mix(in srgb, var(--card) 55%, var(--popover) 45%); }
   ```
   That `color-mix` is a bug report in CSS form. This plan replaces it with a real token.

**The distinctive UE traits worth stealing** (beyond the values): inputs and wells are **recessed *below* the
panel** (inverting the web convention where inputs are lighter); hover is a **big, confident jump**, not a whisper;
and depth is **flat and value-driven**, not shadow-driven.

---

## Audit — what will and won't follow a token change

**Good news: the Tailwind layer is clean.** Zero `bg-zinc-*`/`bg-neutral-*`/`bg-slate-*`/`bg-gray-*` anywhere.
All 7 `dark:bg-*` uses are token-derived shadcn primitives. A token change propagates almost everywhere in
`sentinel-app` for free.

**Surface class usage** (`sentinel-app/src`): `bg-muted` **53×** (the real workhorse) · `bg-card` 35× ·
`bg-background` 29× (mostly as `/70`–`/85` scrims) · `bg-sidebar` 10× (only `routes/+layout.svelte`) ·
`bg-accent` 9× · `bg-popover` 8× · `bg-well` 7× · `bg-secondary` 5× (nearly dead).

**The four things that will NOT follow a token change:**

1. **`sentinel-app/src/lib/map/palette.svelte.ts`** — a whole parallel, JS-side dark ramp that reads
   `theme.current` and switches in JS, so CSS tokens never reach it. Its `floor: #13171b` (≈ L 0.21) sits inside a
   `bg-well` (0.04) container — a 0.17 jump the token system doesn't know about. Worse, its `slateLine #3a4752`,
   `slateFill #1d252d`, `label #7a8c96` and `glow` are **blue-tinted (non-zero chroma)** — a standing violation of
   the "no hue on surfaces" brand rule. Consumers: `lib/three/map/{GroundGrid,MarkerGlyph,RoomMesh,ShapeLayer,WallSegment}.svelte`, `textures.ts`.
2. **`landing/src/app.css`** — forks the *effect* values as literals (`oklch(1 0 0 / 0.2)` for `.interactive`
   border, the lift shadow, `.glow`, `status-pulse`, the shimmer, the `.text-grad` stops) instead of using the
   shared `--lift-*`/`--glow-*`/`--selection`/`--shimmer`/`--pulse-color`/`--grad-*` tokens. Changing dark effects
   updates the app and silently **not** landing. Its `@theme inline` is also a **subset** — `well`, `popover`,
   `secondary`, `accent`, `sidebar` aren't registered, so those Tailwind classes don't exist on landing.
   Landing is **permanently dark** (`landing/src/app.html:2` is `class="dark"`), so it's the showcase for this work.
3. **`landing/src/routes/+page.svelte`** — raw `oklch(0.11 0 0)` (L210) and `oklch(0.16 0 0)`/`oklch(0.08 0 0)` (L289)
   surfaces that sit *between* our ramp stops.
4. **`sentinel-app/src/lib/three/map/DrawPreview.svelte`** — 11 theme-blind literals (`#e5e5e5`, `#d4d4d4`,
   `#b0b0b0`) tuned for the dark floor; already near-invisible on light. Latent bug, in scope to fix while we're here.

**Deliberate exceptions, confirmed clean — do not touch:** `sentinel-app/src/lib/workflows/portColors.ts` (47
categorical port hues, no surfaces, documented theme-independent) and `sentinel-app/src/lib/map/palette.ts` (4
fixed emerald live/interactive accents, no surfaces).

**Editor chrome to reconcile:** `sentinel-app/src/app.css:198-219` (`--wf-*`). `--wf-port-empty` is
`oklch(0.14 0 0)` — hand-tuned to sit just above `--card` (0.13) so empty sockets read as holes *against the node
body*. Move `--card` and sockets stop reading. Same fragility in `--wf-node-glow` (0.90) and `--wf-node-shadow`.

**Deepest-nested screens (the ones that need the new levels):**
1. **Workflow editor** (`routes/workflows/[[id]]/+page.svelte`, 4247 lines, 26 surface classes) — five deep:
   `bg-background` → `bg-well` canvas → `.node-body` color-mix (~0.20) → inspector `bg-card/40` → `bg-card` rows →
   `bg-muted` chips, with `bg-popover/95` menus on top. Also a `bg-well` console nested *inside* a `bg-card/40`
   panel floating on a 0.07 page.
2. **Map** (`routes/map/+page.svelte`) — `bg-well` container → 3D floor `#13171b` (its own ramp) → `SidePanel` →
   `ShapeProps`, plus four `bg-background/75` backdrop-blur HUDs.
3. **Chat** (`routes/chat/+page.svelte`) — `bg-muted/20` scroller → `bg-card` bubbles → `bg-well` `<pre>`.
4. **Layout chrome** (`routes/+layout.svelte`) — `bg-sidebar` rail + `bg-muted/60` subheader over `bg-background`.

**Scrims are the fragile part.** ~22 `backdrop-blur` surfaces, all alpha-over-`--background`. The editor and map
HUDs are `bg-background/75` floating over the **`bg-well` canvas**, not over `--background` — so the alpha maths is
*already* wrong (a 75%-alpha 0.07 surface over a 0.04 well). Re-ramping forces us to fix this properly.

---

## Phase 1 — Define the ramp (`styles/sentinel-tokens.css`)

Rewrite the `.dark` block around a **named elevation scale**, with the shadcn tokens mapped onto it rather than
holding raw values. One source of truth; semantic tokens become aliases.

```
--elev-0  recessed  — inputs, wells, code, log output, canvas, scrollbar track
--elev-1  canvas    — page base
--elev-2  chrome    — sidebar rail, headers, subheader bar
--elev-3  panel     — cards, sheets, the default surface
--elev-4  raised    — nested card-in-card, node bodies, inspector panels  ← the missing stop
--elev-5  float     — popover, dropdown, tooltip, command palette
--elev-6  hover     — interactive hover surface                            ← new
--elev-7  active    — selected / pressed / active item                     ← new
```

- Target the UE band (roughly 0.145 → 0.46), pending **Open decision 1** on how far we lift the floor.
- Map: `--well`→elev-0, `--background`→elev-1, `--sidebar`→elev-2, `--card`→elev-3, `--popover`→elev-5,
  `--accent`→elev-7.
- **Differentiate `--muted` from `--secondary`** — they're identical (0.17) today. `bg-muted` (53×) is the de-facto
  primary surface, so it should land on a real, deliberate stop; `--secondary` (5×) either gets its own or is retired.
- **Register every new token in *both* `@theme inline` blocks** — `sentinel-app/src/app.css` and
  `landing/src/app.css` — or the new classes silently don't exist on landing.
- Light theme keeps a *mirrored structure* (same 8 semantic names, light values) so components can reference
  `--elev-*` unconditionally — see **Open decision 4** on whether we re-tune light's values in this pass.

## Phase 2 — Fix the depth mechanics

The effects that are currently no-ops on near-black:

- **`--lift-shadow` / `--glow-shadow`** — a black shadow on a 0.07 canvas renders nothing. With the canvas lifted
  to UE territory, black shadows start working again; re-tune them, and lean on **brightened borders** (`--lift-border`)
  as the primary lift signal, UE-style.
- **`--overlay` (new token)** — `sentinel-app/src/lib/components/ui/dialog/dialog-overlay.svelte:15` hardcodes
  `bg-black/10`, which on a near-black page means the modal barely separates. Tokenize it and give dark a real scrim.
- **Scrim rebase** — the editor/map HUDs use `bg-background/75` while floating over `bg-well`. Point them at a
  surface that matches what's actually behind them (a `--hud` alias on elev-2/3, or solid + border).
- **`--video-well` (new token)** — the three legitimate `bg-black` video letterboxes
  (`routes/live/+page.svelte:274`, `routes/recordings/+page.svelte:420`,
  `routes/workflows/components/RegionPointsEditor.svelte:94`). Once the floor lifts, true black becomes a *hole*.

## Phase 3 — Recessed inputs + confident hover (the UE traits)

- **Inputs sink.** Adopt UE's inversion: input/textarea/select surfaces sit at **elev-0**, *below* the panel they
  live on, with a hairline lifted border. Touches the shadcn primitives (`ui/input`, `ui/textarea`,
  `ui/select-trigger`) which currently use `dark:bg-input/30`. Gated on **Open decision 3**.
- **Hover is a real step.** Introduce `--hover` (elev-6) / `--active` (elev-7) and route `.interactive` and row/card
  hovers through them, instead of the current `bg-muted/40` ad-hoc alpha.

## Phase 4 — Reconcile the workflow editor chrome

- Retune `--wf-*` in `sentinel-app/src/app.css:198-219` against the new ramp — especially `--wf-port-empty` (must
  read as a hole *against the node body*, not against `--card`), `--wf-node-glow`, `--wf-node-shadow`.
- **Delete the `color-mix` hack** at `routes/workflows/[[id]]/+page.svelte:4230` — `.node-body` becomes
  `var(--elev-4)`. This is the plan's canary: if the node body looks right with a plain token, the ramp is correct.
- Canvas (`bg-well`) vs node body vs inspector must now be three legible steps. Port hues in `portColors.ts` stay untouched.

## Phase 5 — Reconcile the map

- **`lib/map/palette.svelte.ts`** — kill the parallel ramp. Either read the CSS custom properties at runtime
  (`getComputedStyle`) or, simpler, re-derive `DARK`/`LIGHT` from the same numeric scale so the two can't drift.
- **De-tint the blue cast** — `slateLine #3a4752`, `slateFill #1d252d`, `label #7a8c96`, `glow "160,185,200"` carry
  chroma on *surfaces*, against the brand rule. Resolve once via **Open decision 2**.
- Re-anchor `floor` (today `#13171b` ≈ L 0.21) as a deliberate stop on the ramp, and fix the `bg-well` container it
  sits in so the 3D floor doesn't jump 0.17 levels against its own frame.
- Fix the theme-blind literals in `lib/three/map/DrawPreview.svelte` (`#e5e5e5`/`#d4d4d4`/`#b0b0b0`).
- Map's emerald live/interactive accents (`lib/map/palette.ts`) stay — they're state, not surface.

## Phase 6 — Landing (the dark showcase)

- **De-fork `landing/src/app.css`** — replace the literal effect values with the shared `--lift-*`, `--glow-shadow`,
  `--selection`, `--shimmer`, `--pulse-color`, `--grad-*` tokens, per the tokens file's own "add effects here, never
  per-file" rule.
- Extend landing's `@theme inline` to register the full ramp.
- Replace the raw surfaces in `landing/src/routes/+page.svelte` (`oklch(0.11 0 0)` L210;
  `oklch(0.16 0 0)`/`oklch(0.08 0 0)` L289) with ramp tokens.
- Landing is dark-locked, so it's where the new palette is judged first.

## Phase 7 — Sweep, verify, ship

- Walk the deepest-nested screens in order — workflow editor, map, chat, layout chrome, logs — against the
  `/impeccable` craft checklist (states, motion, hierarchy, depth, consistency).
- `routes/logs/+page.svelte` module dots: `workflow: "#e5e5e5"` is near-white and invisible on light; fold into the
  pass.
- `npm run build` + `task check`. Eyeball both themes on every screen; light must not regress.

---

## Decisions taken (2026-07-12)

1. **Floor: Faithful Unreal.** Adopt UE's band wholesale — nothing darker than 0.145, including the editor
   canvas and code blocks. Sentinel Noir becomes graphite; accepted. Ramp below.
2. **Chroma: 0.** Surfaces stay pure neutral, per the brand rule. UE's Slate values are themselves neutral —
   the remembered "UE blue-grey" comes from its blue accent, which we don't have. The map's blue-tinted
   surfaces get de-tinted.
3. **Inputs sink.** Inputs/textareas/selects sit at `--elev-1`, *below* the panel, with a hairline lifted border.
4. **Light theme: restructured, not re-tuned.** Light gets the same semantic `--elev-*` role names and the new
   tokens, but keeps its current values. Avoids doubling the review surface.
5. **`--secondary` kept, given a real stop** (`--elev-5`) rather than retired — shadcn's `secondary` button
   variant needs the token, and 5 call sites isn't worth a migration.

### The ramp (dark)

| Token | L | UE anchor | Role |
|---|---|---|---|
| `--elev-0` | 0.145 | `#0A0A0A` | recessed well — editor canvas, code, log output, scrollbar track |
| `--elev-1` | 0.170 | `#0F0F0F` | sunken — **inputs**, sidebar rail, headers |
| `--elev-2` | 0.196 | `#151515` | base — page canvas |
| `--elev-3` | 0.235 | `#1E1E1E` | panel — cards, the default surface |
| `--elev-4` | 0.265 | `#242424` | raised — node body, nested panel, chips (`--muted`) |
| `--elev-5` | 0.305 | — | float — popover, dropdown, tooltip, `--secondary` |
| `--elev-6` | 0.365 | `#383838` | hover — `--accent`, interactive hover |
| `--elev-7` | 0.455 | `#575757` | active — selected / pressed |

Note the UE trait preserved in the mapping: **chrome recedes**. The sidebar sits at `--elev-1` (0.170),
*darker* than the page canvas at `--elev-2` — exactly as UE's title bars (`#0F0F0F`) sit below its
background (`#151515`). Light theme inverts this (its rail is near-white and advances), which is why the
`--elev-*` scale is defined per-theme by **role**, not as a cross-theme lightness ordinal.

## Open decisions (resolved above — kept for the record)

1. **How far do we lift the floor?** This is *the* decision — everything else follows.
   - **(A) Faithful UE band** — everything moves into 0.145–0.46. Truest to the ask; the app reads as a
     professional tool. But it's a big visible shift: Sentinel Noir stops being *noir* and becomes *graphite*.
   - **(B) Conservative** — keep the deep 0.04/0.07 floor, just fill the missing `--card`→`--popover` gap and split
     `muted`/`secondary`. Low risk, but leaves the invisible-shadow defect and doesn't really deliver "like Unreal".
   - **(C) Hybrid — my recommendation.** Lift the *interactive* stack into UE territory (canvas ≈ 0.17–0.19, panel
     ≈ 0.24, float ≈ 0.32, hover ≈ 0.40+) so depth and shadows work, while keeping a genuinely deep `--elev-0`
     (≈ 0.10–0.12, below UE's 0.145) for true insets — the editor canvas, code blocks, log output, video wells.
     That preserves the Noir signature where it's dramatic (the big dark canvas) and buys real legibility
     everywhere else.
2. **Chroma: stay at 0, or a whisper of cool?** UE's Slate values are genuinely neutral — the "UE blue-grey" people
   remember comes from its **blue accent**, which we don't have. Brand rule says no hue on surfaces, and I'd
   **keep chroma 0**. But the map *already* ships blue-tinted surfaces, so decide once and apply everywhere. (If we
   do want a whisper, ~`0.004` chroma at hue ≈ 250 is the ceiling before it reads as "blue".)
3. **Do inputs sink (UE) or float (web/shadcn convention)?** UE puts inputs *below* the panel. It's a strong,
   distinctive signal — but it inverts what shadcn ships and touches every form in the app.
4. **Does the light theme get re-tuned in this pass**, or only re-structured onto the `--elev-*` names with its
   current values? Re-tuning doubles the review surface; re-structuring alone is cheap and unblocks the dark work.
5. **Retire `--secondary`?** 5 uses, identical to `--muted`. Either give it a real distinct stop or delete it and
   migrate those 5 call sites.

## Out of scope (v1)

- `lib/workflows/portColors.ts` categorical port/category hues — deliberate colour identity, untouched.
- `lib/map/palette.ts` emerald live/interactive accents — state colour, untouched.
- Semantic state colours (destructive red, amber armed) — unchanged.
- A full light-theme re-tune, unless Open decision 4 says otherwise.
- Any new components. This is a token + reconciliation pass, not a redesign of screens.
