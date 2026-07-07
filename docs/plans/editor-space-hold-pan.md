# Space-hold temporary pan (V/H) — Workflow + Map editors — Implementation Plan

**Board card:** #60 Editor keybinds: hold Space to pan (temp H) (ToDo)
**Goal:** While in Select (V) mode, holding **Space** temporarily switches the editor to Hand/Pan
(H) for as long as the key is held, then snaps back to the prior tool on release — so you can
pan quickly without toggling tools. Applies to the **workflow editor** and the **map editor**.

## Behaviour spec (both editors)
- **Space down** (not typing in a field, not already panning-by-space): remember the current
  tool, switch to pan. Cursor shows `grab` / `grabbing`.
- **Hold + drag**: pans the canvas exactly like the H tool / empty-background pan does today.
- **Space up**: restore the remembered tool. If a pan drag is in flight when Space is released,
  let the drag finish (or cancel cleanly) and still restore the tool.
- **Key repeat**: `keydown` fires repeatedly while held — only act on the first (guard with a
  boolean flag so we don't overwrite the remembered tool with `"pan"`).
- **Don't hijack Space while typing**: reuse each editor's existing "typing into a field" guard.
  Space must still type a space in inputs/textareas/contentEditable, node labels, comment text,
  the map text tool, etc.
- **Lost focus / blur**: on `window` blur (alt-tab mid-hold) force-release so we don't get stuck
  in pan mode. Also call `preventDefault()` on Space-down over the canvas to stop the page from
  scrolling.

## Phase 1 — Workflow editor (`src/routes/workflows/[[id]]/+page.svelte`)
Cleanest case: everything already keys off `editorMode` (`"move" | "pan" | "link" | "break"`,
line ~220). `startDrag`, `onCanvasDown`, and `canvasCursorClass()` already branch on
`editorMode === "pan"`, so we only need to drive that state from Space.

- Add session state near `editorMode`: `let spacePanPrev: typeof editorMode | null = null;`
- In `onWindowKey` (handler ~line 1655, already gated to `view === "editor"` and skips when
  `inField`): before the `toolKey` block (~line 1669), handle `e.key === " "`:
  - if `spacePanPrev === null && editorMode !== "pan"`: `spacePanPrev = editorMode;`
    `editorMode = "pan"; pending = null;` then `e.preventDefault();`
  - if already set, just `preventDefault()` (swallow key-repeat).
- Add a keyup path. The window binding at line ~2430 is `onkeydown` only — add
  `onkeyup={onWindowKeyUp}` to `<svelte:window>` and write `onWindowKeyUp`: on `e.key === " "`
  and `spacePanPrev !== null`, restore `editorMode = spacePanPrev; spacePanPrev = null;`.
- Add `onblur` on `<svelte:window>` (or a `blur` listener) that force-releases the same way.
- No changes needed to drag/pan/cursor plumbing — they already respond to `editorMode === "pan"`.

## Phase 2 — Map editor (`src/lib/map/` + `MapScene.svelte`)
Harder: the map has **no pan tool**. `ToolMode` is `select | wall | line | marker | text | rect
| triangle | circle` (`src/lib/map/tools.svelte.ts:6`), and panning is implicit — an
empty-background drag in `select` mode (`MapScene.svelte:onDown` ~line 198). So we can't just
flip a mode; add an explicit **force-pan flag** consulted at the top of `onDown`.

- Add reactive flag on `ToolState` (`tools.svelte.ts`): `spacePan = $state(false)` (a `$state`
  field so cursor styling can react).
- Space key handling in `src/lib/map/keyboard.ts` `handleMapKey` (guarded by `typingInto()`,
  line ~15): on `" "` set `tools.spacePan = true` and return handled (swallow repeat if already
  true). The page listener (`src/routes/map/+page.svelte` ~line 98) is keydown-only — **add a
  `keyup` listener** that clears `tools.spacePan = false` (route through a small
  `handleMapKeyUp` in `keyboard.ts`, or inline). Also add a `window` `blur` handler that clears
  it.
- In `MapScene.svelte` `onDown` (~line 74), **before** the tool branches (line ~76): if
  `tools.spacePan` (and left button), short-circuit to `drag = { mode: "pan", sx: e.clientX, sy:
  e.clientY }` and return — regardless of current tool or what's under the cursor. `onMove`
  already handles `drag.mode === "pan"` via `viewport.panByPixels` (~line 225), and `onUp`
  clears the drag, so no other pointer changes.
- Cursor: the map canvas currently has no cursor styling. Set `renderer.domElement.style.cursor`
  to `grab` when `tools.spacePan` (and `grabbing` while the pan drag is active), reset to `''`
  otherwise — a small `$effect` in `MapScene.svelte` keyed on `tools.spacePan` + `drag`.
- Optional polish: reflect the temporary hand state in the tool palette (`+page.svelte` ~line
  229) — e.g. highlight nothing but show a subtle "Pan" affordance — deferred to keep v1 small.

## Open decisions (grill before building)
1. **Map: add a real `hand` tool too, or only Space-hold?** The workflow editor already exposes
   H as a persistent tool; the map only has implicit pan. v1 does Space-hold only (matches the
   ask). Adding a persistent H tool to the map palette is a separate, larger change — defer
   unless you want parity now.
2. **Restore-to-previous vs always-restore-to-select?** Plan restores the *previous* tool
   (correct for workflow where H/L/B are real tools). For the map, Space is only meaningful from
   `select` anyway, so behaviour is identical either way.
3. **Middle-mouse pan too?** Common in editors (MMB-drag pans in any tool). Cheap to add
   alongside since both editors already have a pan drag path. Include, or defer? (Leaning
   defer — not in the ask.)

## Out of scope (v1)
- A persistent Hand tool + palette button in the map editor.
- Middle-mouse-button pan.
- Space-to-pan in any other canvas (chat, etc.).
- Touch/trackpad two-finger gesture changes.
