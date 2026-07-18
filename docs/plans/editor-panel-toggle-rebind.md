# Editor panel toggle — rebind off Tab — Implementation Plan

**Board card:** Editor panel toggle — rebind off Tab (ToDo)
**Follows:** [[editor-panel-tab-toggle]] (card #61, shipped) — this changes the key that feature chose.
**Goal:** Move the "collapse/expand editor chrome" gesture off **Tab** onto a dedicated key, in
both editors, so Tab is left entirely to the browser (form-field / focus cycling).

## Why
#61 bound **Tab** to toggle the editor chrome, guarded so it only fires when focus is *not* on a
control (`inField` + `onControl()`). But Tab is the browser's focus-navigation key — when focus
rests on a non-focusable element (e.g. after clicking the canvas), Tab still gets intercepted and
`preventDefault`'d, so it no longer cycles form fields / focusable elements as users expect.
Rebinding to a non-Tab key removes the ambiguity completely — no guard gymnastics needed.

## Current state (both editors bind + preventDefault Tab)
- **Workflow** — `sentinel-app/src/routes/workflows/[[id]]/+page.svelte`, `onWindowKey(e)` L1763,
  Tab branch **L1784–1790** → `toggleChrome()` (L384–403, group toggle of rail `panelOpen` +
  console `consoleOpen`, with in-session memory). `e.preventDefault()` at L1787.
- **Map** — `sentinel-app/src/lib/map/keyboard.ts`, `handleMapKey` L33, Tab branch **L38–44** →
  `togglePanel?.()` (flips `panelOpen`, bound into `SidePanel.svelte`). `e.preventDefault()` at L41.
- **Shortcut overlay** — `sentinel-app/src/routes/map/components/ShortcutsOverlay.svelte:16`
  lists `["Tab", "Show / hide the panel"]`. Must be updated or it lies. (Workflow editor has no
  shortcuts overlay.)
- No central keybinding map — bindings are ad-hoc per page (`if (e.key === …)` chains).

## Phase 1 — Rebind the key (both editors)
- Workflow `+page.svelte` L1784: change `if (e.key === "Tab")` to the chosen key. **Keep** the
  `inField` guard (so typing is unaffected); **drop** the now-pointless `onControl()` early-return
  — a non-Tab key doesn't need to yield to focus navigation, so the toggle can fire regardless of
  which element is focused (still after `inField`, so it never fires mid-typing). `toggleChrome()`
  logic is unchanged.
- Map `keyboard.ts` L38: same — swap `"Tab"` for the chosen key, keep `typingInto()`, drop the
  `onControl()` early-return. `togglePanel` unchanged.
- Leave everything Tab used to interfere with alone — Tab now falls through to the browser in both
  editors (no code claims it anymore).

## Phase 2 — Update the labels
- `ShortcutsOverlay.svelte:16`: replace `"Tab"` with the chosen key's glyph.
- Grep for any other user-facing "Tab" hint referencing this gesture and update it.

## Open decisions (grill before building)
1. **Which key?** Recommend **backtick `` ` ``** — free in both editors, and a well-worn
   convention for "toggle panel/console" (VS Code uses Ctrl+`). Alternatives that are also free in
   both: `\`, `.`, `p`. Note `c`/`C` is taken in the map (cycle-selection, moved there by #61) but
   free in the workflow editor — avoid it to keep the gesture's key identical across both editors.
   → **Default if undecided at implement time: backtick.**
2. Keep the gesture **identical in both editors** (one key everywhere)? Assumed yes — that was the
   whole point of #61. If the workflow and map should diverge, say so.

## Out of scope (v1)
- A central/remappable keybinding system. Still ad-hoc per page; extracting it is separate.
- Any on-screen "press ` to toggle" hint beyond the existing map shortcuts overlay.
