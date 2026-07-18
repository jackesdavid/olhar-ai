# Editor chrome-toggle — visible toolbar button — Implementation Plan

**Board card:** Editor chrome-toggle — visible toolbar button (ToDo)
**Related:** [[editor-panel-tab-toggle]] (#61, shipped — added the gesture), [[editor-panel-toggle-rebind]] (#105, rebinds its key)
**Goal:** Give the "collapse/expand editor chrome" group-toggle its own icon button in the editor
toolbar, like the other tools, so tablet/touch users (no keyboard) can tap it — today it's
keyboard-only.

## Why
The group-collapse (rail + console together) is triggered only by a key (`toggleChrome()`, Tab
today / rebinding under #105). Tablet users have no way to invoke it. The per-panel
`PanelRightClose`/`PanelRightOpen` buttons exist but each toggles only *one* panel — there's no
visible affordance for the whole-chrome group toggle in either editor.

## Grounding (`sentinel-app/src/routes/workflows/[[id]]/+page.svelte` unless noted)
- Floating controls are absolutely-positioned `<div>`s layered over the SVG (not SVG elements),
  inside the `relative` editor column (L2810), starting at the `<!-- floating controls -->` comment
  **L3335**. Existing clusters: Add-node (L3336), left tool palette (L3343–3362), transport
  (L3378), zoom cluster top-right (L3400–3408).
- **House style** — cluster wrapper (zoom, L3401): `class="absolute … flex items-center gap-1
  rounded-lg border border-border bg-hud p-1 shadow-lg backdrop-blur"`. Single icon button (L3407):
  `<Button variant="ghost" size="icon" class="size-8" title="…" onclick={…}><Icon class="size-4" /></Button>`.
  Active-state pattern (tool palette, L3351–3360): `class="size-8 {active ? 'bg-primary/15 text-primary' : ''}"`.
- **Handler to call:** `toggleChrome()` **L384–403** (group toggle w/ `preTabPanels` memory). State
  it reflects: `panelOpen` (L366), `consoleOpen` (L247).
- **Icons:** lucide, one-per-file from `@lucide/svelte/icons/*` (L15–64); `Button` from
  `$lib/components/ui/button` (L68). `PanelRight*`/`Chevron*` already imported; `PanelLeft*`/`Sidebar`
  are **not** — a distinct glyph (e.g. `PanelLeftClose` or a maximize-canvas icon) avoids confusion
  with the per-panel `PanelRight*` buttons.
- **Touch already handled:** `isTouch` (L226) from `matchMedia("(pointer: coarse)")` (L1323); the
  tool palette exists specifically for touch parity. Buttons are `size-8` (32px) — matches house
  style (below the 44px ideal, but consistent).
- **Map equivalent:** `routes/map/components/SidePanel.svelte` already has visible per-panel
  collapse buttons (`toggle()` L63–72), but no group-collapse button either — the map has a single
  panel so its `PanelRight*` buttons are 1:1.

## Phase 1 — Add the button to the workflow editor toolbar
- Add one icon `Button` (house style above) to a floating cluster — cleanest home is the top-right
  zoom cluster (L3400–3408) or the left tool palette (L3343–3362). It calls `toggleChrome()`.
- Reflect state: when the chrome is collapsed, show the active/`bg-primary/15` style (and/or swap
  the icon) so the button reads as a toggle, driven by `panelOpen`/`consoleOpen`.
- Pick a glyph distinct from the existing `PanelRight*` per-panel buttons.

## Phase 2 — Mirror in the map editor (consistency)
- The map's group toggle is also keyboard-only. If #105 gives the map a single-panel toggle only,
  this may be a no-op there; otherwise add the same button to the map toolbar. Decide with #105.

## Open decisions (grill before building)
1. **Where does the button live** — top-right zoom cluster, the left tool palette, or its own
   mini-cluster? Recommend the **zoom/view cluster**, since "collapse chrome" is a view action.
2. **Which icon.** Recommend `PanelLeftClose` / a maximize-canvas glyph — clearly not the
   per-panel `PanelRight*`. Needs an impeccable pass.
3. **Bigger hit target on touch?** Existing buttons are 32px. Consider bumping to ~44px when
   `isTouch`, since the whole point is tablet users. Design call.

## Out of scope (v1)
- Reworking the per-panel `PanelRight*` buttons.
- A full responsive/tablet layout pass on the toolbars.
