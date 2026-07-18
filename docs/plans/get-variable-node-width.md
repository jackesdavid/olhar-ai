# Get/Set Variable node — width fits the variable name — Implementation Plan

**Board card:** Get/Set Variable node — width fits the variable name (ToDo)
**Goal:** Stop clipping the header of Get/Set Variable nodes — "Get current_recipient" overflows
because the node is pinned to a fixed 120px regardless of the variable name.

## Root cause
`sentinel-app/src/routes/workflows/[[id]]/+page.svelte`, `nodeW(n)` at **L1036–1041**:
```js
function nodeW(n) {
  if (n.kind === "get_variable" || n.kind === "set_variable") return 120; // ← fixed, ignores title
  const title = nodeTitle(n, catalog[n.kind]);
  const est = 46 + Math.ceil(title.length * 6.6);
  return Math.min(NODE_W_MAX, Math.max(NODE_W, est));
}
```
Every other node kind grows to fit its title via `est`. Get/Set Variable short-circuit to 120px
before reaching it (comment at L1032–1034 says they were deliberately made "narrower"). The
header title is built in `nodeTitle()` (L936–940) as `Get ${config.variable}` and rendered as an
SVG `<text>` at L3074–3082 with **no ellipsis/clip** — so a long variable name just overflows the
node body rect. Width is computed in JS (not stored on the model, unlike comment nodes), and
`n.config.variable` is already available at render time, so this is a pure frontend one-liner.

## Phase 1 — Let Get/Set Variable size to their title
- In `nodeW()` (L1037), remove the fixed `return 120` short-circuit so these kinds fall through to
  the same title-based `est` sizing used by every other node.
- The `est`/`NODE_W`/`NODE_W_MAX` clamp (L1038–1040) then handles both short names (min width) and
  long ones (capped at `NODE_W_MAX = 320`). `nodeTitle()` already yields the exact on-canvas
  string, so the estimate matches what's drawn.

## Open decisions (grill before building)
1. **Keep a tighter minimum for these two kinds?** They were intentionally narrow. Options: (a)
   drop the special case entirely → they use the standard `NODE_W = 176` min like everything else;
   (b) keep a smaller floor (e.g. `Math.max(120, est)`) so a `Get x` stays compact but a
   `Get current_recipient` grows. Recommend **(b)** — preserves the compact look for short names,
   fixes the clip for long ones.
2. Same treatment for **Set Variable** (it shares the branch and the same title shape) — assumed
   yes; both fixed together.

## Out of scope (v1)
- Ellipsis/`textLength` truncation on the SVG title (we're widening to fit, not truncating).
- Storing width on the node data model / making these nodes resizable (that's the comment-node
  pattern, `commentW`/`n.config.width` — not needed here).
- Re-measuring via the DOM; the existing character-count estimate is the established pattern.
