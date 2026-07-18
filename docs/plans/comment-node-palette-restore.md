# Comment node missing from add-node palette — Implementation Plan

**Board card:** Comment node missing from add-node palette (ToDo)
**Related:** [[comment-node]] (#38, shipped the node)
**Goal:** Make the Comment node reachable again — it "disappeared" because the palette filters it
out unconditionally, and it has no other add affordance.

## Root cause (a regression)
`sentinel-app/src/routes/workflows/components/ContextSearch.svelte:39` drops Comment from **every**
palette invocation:
```js
.filter((k) => k.slug !== "comment" && (!q || k.label.toLowerCase().includes(q) || k.slug.includes(q)))
```
The `k.slug !== "comment"` clause removes it from the general "Add node" menu too (the `n` key and
Add button, where `portType === "any"`). Comment has **no** toolbar button / double-click /
right-click add path — `ContextSearch` is the only way in — so this makes it unreachable.

The two layers now contradict each other:
- `+page.svelte:614–619` deliberately **keeps** `comment` in `menuKinds` (`HIDE_FROM_MENU` at L615
  is `get_variable`/`set_variable`/`reroute` only), with a comment: *"`comment` IS listed — it has
  no other add path."* (added 2026-07-02, `1ab306d3`).
- `ContextSearch.svelte:39` strips it right back out (present since the component was extracted in
  `45642a0`, survived `2abacb5`/`2d761f2`).

Excluding Comment is only correct for the **wire-drop** case (`portType !== "any"`) — it has no
input ports to connect to. In the general menu it should show.

Confirmed present in the catalog: `NodeKind::Comment` in `domain.rs` (variant L418, in `ALL` L910,
slug `"comment"` L981, label `"Comment"` L1056, category `"Debug"` L1126). `ContextSearch` renders
a flat score-sorted list (no category grouping), so category doesn't affect visibility.

## Phase 1 — Make the exclusion port-context-aware
- `ContextSearch.svelte:39`: gate the `comment` exclusion on port context — only drop it when
  `portType !== "any"` (wire-drop), keep it in the general add menu. `portType` is already a prop
  (L22/28) and the "any" vs typed distinction is used elsewhere (L91). E.g. change the guard to
  `(portType === "any" || k.slug !== "comment")`.
- Verify via the three open paths: `n` key (`+page.svelte:1802–1809`), Add button
  (`openAddNodeMenu`, `+page.svelte:2507–2515`) → both `portType === "any"` → Comment visible;
  wire-drop (`+page.svelte:2355–2356`, concrete port type) → Comment hidden.

## Open decisions (grill before building)
1. **Cosmetic:** `domain.rs:1126` puts Comment in category `"Debug"`; the original plan
   ([[comment-node]] §2e) intended `"Utility"`. Only affects the palette glyph/color, not
   visibility. Fix while here, or leave? Recommend fixing to match the plan — one-line change.

## Out of scope (v1)
- A dedicated "Add comment" toolbar/context-menu affordance (the palette is the intended path).
- Any change to how Comment renders/behaves on the canvas.
