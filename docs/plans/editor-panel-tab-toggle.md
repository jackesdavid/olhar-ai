# Editor panel Tab-toggle — Implementation Plan

**Board card:** #61 Tab toggle: collapse/expand editor side panels (In Progress)
**Goal:** Pressing **Tab** in the Workflow editor collapses/expands its chrome as a group, giving the canvas the
whole window in one keypress; pressing **Tab** in the Map editor collapses/expands its side panel. Same gesture,
both editors.

**Decisions locked in** (from the user, still hold):
- Workflow editor: Tab collapses/expands **all** its panels **as a group**, not one at a time.
- Map editor: **move `cycleSelection` off Tab onto a new key** so Tab means the same thing in both editors.

## Re-grounded after #75 (this plan was written against the old layout)

The original plan assumed **three** panels in the workflow editor — a left Variables rail, the right config
panel, and the bottom console. **Card #75 deleted the left rail**: Variables is now a tab inside the single
right rail (`[[workflow-editor-tabbed-inspector]]`). So `varsOpen` **no longer exists**, and "all three panels"
is now **two**:

| Panel | State | Persisted? |
|---|---|---|
| The right rail (Details/Properties/Variables/Runs) | `panelOpen` (`+page.svelte:~350`), `togglePanel()` | yes — `wf.panelOpen` |
| The console (bottom, full width) | `consoleOpen` (`+page.svelte:247`) | **no** — toggled inline at **L4231** |

Everything else in the original grounding still holds:
- Workflow keybinds: `onWindowKey(e)` (**L1724**), gated on `view === "editor"`, with an `inField` guard at
  **L1728/1737** so Tab still moves focus normally inside inputs. **No key is bound to Tab today.**
- Map: `handleMapKey` in `lib/map/keyboard.ts` binds **Tab → `store.cycleSelection()`** (**L60–64**). Free
  letters include `c`/`C` — mnemonic for **c**ycle, and `Shift+C` keeps the forward/back convention.
- Map `SidePanel.svelte` owns `open` **privately** (`$state(true)`, **L23**), persisted to `map.panelOpen` via
  its own `toggle()` (**L40–47**). The page can't reach it — see Phase 2.
- The map's shortcuts overlay lists `["Tab / ⇧Tab", "Cycle selection"]` (`ShortcutsOverlay.svelte:25`) and must
  be updated or it will lie.

## Phase 1 — Workflow editor: Tab collapses/expands the chrome as a group

Tab is a **group toggle with memory**, not two independent toggles:

- Add `preTabPanels: { rail: boolean; console: boolean } | null` next to `panelOpen`/`consoleOpen`.
- On Tab (in `onWindowKey`, after the `inField` guard so typing is unaffected):
  - **If nothing is open** (rail closed *and* console closed, no memory) → open the rail. Otherwise Tab would
    look dead when you press it on an already-bare canvas.
  - **Else if there's no memory** → remember both states, then close both.
  - **Else** → restore both from memory and clear it. A panel you had *already* closed by hand stays closed.
- **Any manual toggle clears the memory** (`togglePanel`, the console header, and `selectTab` — which force-opens
  the rail). Otherwise the next Tab does a confusing partial restore instead of a fresh collapse.
- Persist `consoleOpen` under a new `wf.consoleOpen` key (it isn't persisted today) so the group toggle and the
  manual toggle agree across reloads, and restore it in `onMount` alongside `wf.panelOpen`.
- Replace the console header's inline `onclick={() => (consoleOpen = !consoleOpen)}` (**L4231**) with a real
  `toggleConsole()` that persists and clears the memory.

## Phase 2 — Map editor: free Tab, rebind cycle-selection to C / Shift+C

- `lib/map/keyboard.ts`: delete the `if (e.key === "Tab")` block (**L60–64**); add `case "c": case "C":` to the
  switch → `store.cycleSelection(e.shiftKey ? -1 : 1)`. Same semantics, new key.
- Bind Tab to the panel toggle. `SidePanel` currently owns `open` privately, so **make it `$bindable`** and bind
  from `routes/map/+page.svelte`; the page's key handler flips it. Keep `SidePanel`'s own `toggle()` (and its
  `map.panelOpen` persistence) as the single writer so both paths persist identically.
- The map has one panel, so this is a plain toggle — no group-memory logic.
- Update `ShortcutsOverlay.svelte:25`: `Tab` now = "Toggle panel", and add `C / ⇧C` = "Cycle selection".

## Keyboard-trap guard (added during implementation — not in the original plan)

Tab is **the** focus-navigation key. Claiming it globally would mean a keyboard user could no
longer Tab through the editor's buttons at all — a keyboard trap, and a real a11y regression.

So in **both** editors, Tab is only intercepted when focus is **not on a control**:
- the existing `inField` / `typingInto()` guard already lets Tab through while typing, and
- a new `onControl()` check (`button, a[href], [tabindex]:not([tabindex='-1'])`) lets it through
  while tabbing across buttons and links.

Click the canvas — or land anywhere non-focusable — and Tab is the panel toggle. Focus a control
and Tab moves focus, as it must. This costs nothing and keeps the gesture the card asked for.

## Out of scope (v1)

- A shared `<CollapsiblePanel>` component. Collapse logic is still duplicated (workflow rail, workflow console,
  map SidePanel), but #75 already cut it from four copies to three; extracting it is a separate cleanup.
- Any on-screen "Press Tab" hint.
- Persisting the *collapsed-by-Tab* group state across reloads — the memory is in-session only, and a fresh load
  starts from each panel's own persisted state.
