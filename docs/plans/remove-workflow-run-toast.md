# Remove per-run workflow toast — Implementation Plan

**Board card:** Remove per-run workflow toast (ToDo)
**Goal:** Stop firing a toast every time *any* workflow in the tenant finishes — it's noise.

## Root cause
The toast is a pure frontend behaviour — no backend push is involved.

`sentinel-app/src/routes/workflows/[[id]]/+page.svelte`
- Line 13: `import { toast } from "svelte-sonner";`
- Lines 1350–1366: the `onEvent("workflow-run", …)` SSE handler. Lines 1364–1365:
  ```
  if (ok) toast.success(`Workflow "${name}"`, { description: summary });
  else    toast.error(`Workflow "${name}" failed`, { description: summary });
  ```
  These two lines sit **outside** the `gid === selectedGraphId` guard (lines 1360–1363), so
  the toast fires for every run of every graph in the tenant, on every run. The comment at
  line 1359 states this is intentional ("other graphs' completions still surface as toasts").

## The change
- Delete lines 1364–1365 (the two `toast.*` calls) and the misleading comment at line 1359.
- Run outcomes still surface where they belong: the Console log (lines 1360–1363) for the open
  graph, plus live node highlighting and the activity map — all driven by the same SSE event.
- If `toast` becomes unused in this file after the deletion, drop the import at line 13 too.

## Do NOT touch
- The `workflow-run` SSE emit sites in `sentinel-flow/src/engine.rs` (lines 163–167, 592–596,
  668–672). Those events also drive node highlighting, the activity map, and the console —
  removing them breaks far more than the toast.
- `sentinel-app/src/lib/data.ts` `onEvent("workflow-run", …)` plumbing (lines 120–134).

## Out of scope (v1)
- No opt-in "notify me on run" setting — the ask is to remove the feature, not gate it.
- Failure toasts are also removed (line 1365). If failures should still surface, that's a
  follow-up; the current ask is remove-all.

## Verify
- Run a workflow with the editor open → outcome shows in Console, no toast.
- Run a *different* workflow (not the open graph) → no toast at all.
