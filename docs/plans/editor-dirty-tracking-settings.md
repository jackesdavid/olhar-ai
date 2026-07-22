# Editor dirty-tracking — settings edits don't light Save — Implementation Plan

**Board card:** 🐛 #134 Editor: schedule/group/settings edits don't mark the graph dirty
**Goal:** Every edit in the workflow editor's config surfaces should reflect in the Save
indicator. Changing the schedule trigger, the workflow group, and several other settings
persisted silently but left the Save button showing "Saved", so users couldn't tell their
change registered.

## Root cause
The editor has **two save models** (`sentinel-app/src/routes/workflows/[[id]]/+page.svelte`):
1. **Canvas body** (nodes/edges/positions/viewport/next_id) — explicit Save (⌘S), gated on a
   `dirty` flag (`markDirty()` → `dirty = true`; Save button `disabled={saving || !dirty}`). #122.
2. **Graph settings** (name, description, group, trigger/schedule, concurrency, instance scope,
   run-orphans, debug, variables) — each **auto-persists immediately** via its own endpoint
   (`data.setGraph*` / `renameGraph` / `scheduleVarSave`), deliberately NOT going through the
   canvas Save so a reload can't wipe unsaved canvas edits (#122).

Because model 2 skipped `markDirty()`, those edits never lit the Save affordance. `save()` only
sends the canvas body (`{nodes, edges, viewport, edge_vias, next_id}`), so calling `markDirty()`
on a settings edit is safe — Save never re-sends (or clobbers) the setting; `dirty` becomes a
pure UX signal that "you changed something".

## The change
Add `markDirty()` (after the optimistic local mutation) to every settings mutation of the OPEN
graph: `renameInline`, `saveDescription`, `saveGroup`, `applyTrigger`, `applySchedule`,
`applyConcurrency`, `applyInstanceScope`, `toggleRunOrphans`, `toggleDebug`, `addVariable`,
`removeVariable`, and `editVariable` (moved `markDirty` out of the name/type-only branch so a
default-only edit also marks dirty).

Canvas edits already mark dirty (node drag, comment resize via `saveNodeConfig`, edge add, via
bends via `saveEdgeVias`, `saveNodeConfig`/`liveSaveConfig`/`saveNodeMemoize`/`setSchemaColumns`).

## Deliberately excluded
- **`toggleEnabled`** — arm/disarm is an operational state change (it also locks editing), not an
  unsaved edit; lighting "Save" there would be misleading.
- **`viewport`** — camera pan/zoom; marking dirty would keep Save perpetually lit.
- **`toggleGlobal` / `toggleTemplate`** — list-level template-publishing ops with their own
  optimistic revert, not edits to the open graph's content.

## Verify
- Change schedule times / weekdays → Save lights up. Change the Group field → Save lights up.
- Add/rename/retype/remove a variable, change concurrency / instance scope / description → Save lights up.
- Toggling Enabled (arming) does NOT light Save; panning the canvas does NOT light Save.
