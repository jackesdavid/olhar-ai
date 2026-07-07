# Editor panel Tab-toggle — Implementation Plan

**Board card:** #61 Tab toggle: collapse/expand editor side panels (ToDo)
**Goal:** pressing Tab in the Workflow editor collapses/expands **all three** of its panels
(left Variables, right config, bottom console) together; pressing Tab in the Map editor
collapses/expands its one side panel — in both cases reclaiming full canvas space with a
single keypress instead of clicking each rail button.

**Decisions locked in** (were open questions, now resolved by the user):
- Workflow editor: Tab collapses/expands **all three** panels (left + right + console) as a
  group, not just left+right.
- Map editor: **move `cycleSelection` off Tab onto a new key** so Tab is free for the panel
  toggle and matches the workflow editor's gesture literally on both editors.

## Current state (grounding)

- **Workflow editor** — `sentinel-app/src/routes/workflows/[[id]]/+page.svelte`
  - Col 0, left: `<VariablesPanel bind:open={varsOpen} .../>` (line 2637). `varsOpen =
    $state(true)` (line 1557), not currently persisted to localStorage (only its width is,
    via `wf-vars-w`). Toggle button lives inside `VariablesPanel.svelte`
    (`PanelLeftClose`/`PanelLeftOpen`).
  - Col 2, right: inline config/property panel. `panelOpen = $state(true)` (line 349),
    `togglePanel()` (352-359) flips it and persists to `localStorage["wf.panelOpen"]`.
  - Bottom: console panel. `consoleOpen = $state(false)` (line 243), toggled at its header
    (`onclick={() => (consoleOpen = !consoleOpen)}`, line 3998) — a chevron collapse, not the
    rail-button pattern the other two use. Not persisted to localStorage today.
  - Keybinds: `onWindowKey(e)` (line 1652), gated on `view === "editor"` and an `inField`
    typing guard. No key bound to Tab today.

- **Map editor** — `sentinel-app/src/routes/map/+page.svelte` + `SidePanel.svelte`
  - One right side panel, `SidePanel.svelte`, `open = $state(true)` persisted to
    `localStorage["map.panelOpen"]`, with tabs (`properties`/`explorer`).
  - Keybinds: `handleMapKey(e, ctx)` in `sentinel-app/src/lib/map/keyboard.ts`, gated by
    `typingInto()`. Currently on Tab (line ~60): `store.cycleSelection(e.shiftKey ? -1 : 1)`.
    Used keys elsewhere in that file: arrows (nudge/resize), `[`/`]` (rotate), `?` (help),
    `Cmd/Ctrl+Z` (undo/redo), `Escape`, `Delete`/`Backspace`, `v`, `w`, `m`, `f`. Free letters
    include `c`/`C` — mnemonic for "**c**ycle", matching the old Tab/Shift+Tab forward/back
    convention with Shift.

No shared `<CollapsiblePanel>` component exists — collapse logic is independently duplicated
across `VariablesPanel.svelte`, inline in the workflow `+page.svelte` (x2: right panel and
console), and `SidePanel.svelte`.

## Phase 1 — Workflow editor: Tab collapses/expands all three panels as a group

Behavior: Tab is a **group toggle**, not three independent toggles. When any of the three is
open and Tab is pressed, remember each panel's current open/closed state, then collapse all
three. Press Tab again to restore each panel to the state it was remembered in (so a panel the
user had already closed manually stays closed — it isn't forced open).

- Add `let panelsCollapsedByTab = $state(false);` and a small remembered-state holder, e.g.
  `let preTabPanelState: { vars: boolean; panel: boolean; console: boolean } | null = null;`
  near the existing `panelOpen`/`varsOpen`/`consoleOpen` declarations.
- In `onWindowKey`, add (before other key handling) a case for `e.key === "Tab"`:
  - `e.preventDefault();`
  - if `preTabPanelState === null` (i.e. not currently collapsed-by-Tab): save
    `{ vars: varsOpen, panel: panelOpen, console: consoleOpen }` into `preTabPanelState`, then
    set `varsOpen = panelOpen = consoleOpen = false`.
  - else: restore `varsOpen`, `panelOpen`, `consoleOpen` from `preTabPanelState`, then set
    `preTabPanelState = null`.
  - Persist `panelOpen`/`consoleOpen` to their localStorage keys same as their existing manual
    toggles do (add a `wf.consoleOpen` key alongside `wf.panelOpen`, `wf-vars-open` for parity —
    note `varsOpen`'s open/closed state isn't persisted today; this phase can add that
    persistence too since it's now driven by the same code path).
- Respect the existing `inField` guard so Tab still moves focus normally inside inputs/selects/
  textareas — unchanged from today.
- If the user manually clicks a rail button to reopen one panel while the other two are still
  Tab-collapsed, treat that as leaving the "collapsed-by-Tab" group state (set
  `preTabPanelState = null`) so the next Tab press does a fresh collapse-all rather than a
  confusing partial restore.

## Phase 2 — Map editor: free Tab, rebind cycle-selection to C / Shift+C

- In `handleMapKey` (`sentinel-app/src/lib/map/keyboard.ts`), remove the existing
  `if (e.key === "Tab")` block (~line 59-63).
- Add `case "c": case "C":` to the switch statement: `store.cycleSelection(e.shiftKey ? -1 :
  1);` — same forward/back semantics as before, just off `C`/`Shift+C` instead of `Tab`/
  `Shift+Tab`.
- Add the Tab handling in `sentinel-app/src/routes/map/+page.svelte` (or inside
  `handleMapKey`, matching how the existing panel toggle is wired) to flip `SidePanel`'s
  `open` state and persist to `localStorage["map.panelOpen"]` — the map editor only has the one
  panel, so this is a plain single toggle (no group-remember logic needed like Phase 1).
- Update the in-app shortcuts overlay (bound to `?`, `toggleHelp`) if it lists `Tab: cycle
  selection` today, so the help text matches the new binding.

## Phase 3 — (optional) de-duplicate the collapse pattern

Not required for v1, but worth doing opportunistically while touching all four panels (3 in
workflow + 1 in map): extract the rail-button + collapse/expand markup into a small shared
component so future panels don't re-implement it a fifth time. Defer if it risks scope creep.

## Out of scope (v1)

- The shared `<CollapsiblePanel>` extraction (Phase 3) unless it falls out naturally.
- Any new visual affordance beyond the existing rail buttons / console chevron (no on-screen
  "Press Tab" hint).
- Persisting the "collapsed-by-Tab" group state itself across reloads (Phase 1's memory is
  in-session only — a fresh page load always starts from each panel's own persisted state).
