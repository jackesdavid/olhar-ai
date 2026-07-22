# Split the workflow editor mega-component — Implementation Plan

**Board card:** #137 Refactor `workflows/[[id]]/+page.svelte` (5.5k lines) into components
**Goal:** Break the single 5,570-line `+page.svelte` (script 1–3162, template LIST + EDITOR) into
focused files, with **no behaviour change**. It is the app's most complex screen and has no visual
test harness, so every phase is small and gated on `svelte-check` (baseline: 9 pre-existing errors,
none in this file's new code) + a deploy checkpoint before the next phase.

## Target architecture
`+page.svelte` becomes a thin router on `$page.params.id`:
- no id → `<WorkflowsList/>` (list mode)
- id → `<WorkflowEditor {id}/>` (editor mode)

with the editor further decomposed and pure logic pushed to `$lib/workflows/*.ts`:

```
routes/workflows/[[id]]/+page.svelte      # thin router
routes/workflows/components/
  WorkflowsList.svelte                     # list mode: search/group/tabs + rows + delete/run
  TemplateGallery.svelte                   # the Templates tab grid (own + global)
  WorkflowEditor.svelte                    # editor shell: state owner, wires the two below
  WorkflowCanvas.svelte                    # the SVG canvas: nodes, edges, display cards, drag/marquee
  NodeInspector.svelte                     # right rail: Details/Properties/Variables/Runs
  (existing) ContextSearch / NodePalette / RegionPointsEditor / VariablesPanel
lib/workflows/
  display.ts                               # pure Display-card helpers (wrap/markdown/title sizing)
  portColors.ts (exists)
```

## Phases (each: extract → `svelte-check` → commit → deploy → next)
1. **Pure logic → `lib/workflows/display.ts`** — move the self-contained Display-node helpers
   (`stripInline`, `wrapText`, `mdLines`, `titleFontSize`/`TITLE_SIZES`, `titleW`/`titleH`). No
   reactive state, no template change. **Lowest risk — first.**
2. **`TemplateGallery.svelte`** — the inline Templates-tab grid (props: `templates`, `loading`,
   `usingId`, `onUse`, `onBack`). Self-contained, prop-driven.
3. **`WorkflowsList.svelte`** — the whole list mode (header, tabs, search/group filter, grouped
   rows, run/delete/template toggles, activity dots). It shares only `graphs` + navigation with the
   editor; give it its own data load so `+page.svelte`'s list branch becomes `<WorkflowsList/>`.
4. **`NodeInspector.svelte`** — the right rail (Details/Properties/Variables/Runs + the config field
   chain). Large + prop-heavy; extract behind an explicit prop/callback contract, no state move.
5. **`WorkflowCanvas.svelte`** — the SVG canvas (nodes, edges, display cards, drag/marquee/resize,
   ports, previews). The heaviest; extract last, once the rail is out and the surface is smaller.
6. **`WorkflowEditor.svelte`** — lift the remaining editor state into a shell that owns state and
   composes Canvas + Inspector; `+page.svelte` reduces to the router.

## Constraints / rules
- **No behaviour change** in any phase — pure structure. Verify each with `svelte-check` (same error
  count) and a smoke test in the deployed app before the next phase.
- Prefer **prop/callback contracts** over moving `$state` when coupling is high (phases 4–5), to keep
  each step reversible and reviewable; move state only where a unit is genuinely self-contained
  (phases 2–3).
- Keep the existing `workflows/components/` + `lib/workflows/` conventions.

## Out of scope
- Any UX/logic change (this is a pure refactor; features ride their own cards).
- Rewriting the SVG rendering approach or the state model.
