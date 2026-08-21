# The faces canvas should sit on the workflow editor's dot field

**Board card:** #201 Faces canvas background — match the workflow editor exactly
**Goal:** The Faces canvas and the workflow editor are both infinite pan/zoom surfaces, and they
should read as the same surface. Today they do not.

## What each does now

**Workflow editor** — a DOM element with the dot field in CSS, two radial-gradient layers that
track pan and zoom:

```
radial-gradient(circle at center, var(--wf-grid-strong) …px, transparent 0)  /* 90px lattice */
radial-gradient(circle at center, var(--wf-grid-weak)   …px, transparent 0)  /* 18px lattice */
background-size:     90*zoom, 18*zoom
background-position: pan.x, pan.y
```

The tokens are theme-aware (`app.css`): `--wf-grid-strong` / `--wf-grid-weak` have separate light
and dark values.

**Faces canvas** — an `HTMLCanvasElement` painted in 2D, whose background is a hardcoded fill:

```js
ctx.fillStyle = "#0a0b0d";   // canvas/+page.svelte:252
ctx.fillRect(0, 0, W, H);
```

A literal near-black, no lattice, **and no light theme** — it stays black on Clean White, which is
the part that makes the two surfaces look unrelated rather than merely different.

## Why it is not a one-line change

The editor's grid is CSS on a DOM node; the faces surface is a `<canvas>`. The dot field has to
arrive by one of two routes:

1. **Paint it in the 2D context** — draw the two lattices honouring the same `pan`/`zoom`, reading
   the token values via `getComputedStyle().getPropertyValue("--wf-grid-strong")`. Keeps one
   element; costs a redraw of the field on every frame.
2. **Layer it** — a DOM div carrying the identical CSS background behind a canvas painted
   transparent (`clearRect`, drop the `fillRect`). Zero drawing cost, and the grid is then literally
   the same code path as the editor, which is what "exactly like" should mean.

Route 2 is the honest reading of the request, and it deletes code rather than adding it. The catch
is that pan/zoom must drive the div's `background-position`/`background-size` in step with the
canvas transform, or the field will slide against the islands.

## Also worth fixing while there

The hardcoded `#0a0b0d` is not the only literal — the island fills and label colours are raw
`rgba(...)` too (lines ~269–316). The house rule is tokens, not colour literals in markup or paint.
Whether that is in scope is a judgement call; matching the background is the ask.

## Verification criteria

1. Side by side, the faces canvas and the workflow editor show the same dot field at the same zoom.
2. It follows the theme: light on Clean White, dark on Sentinel Noir — no hardcoded near-black.
3. Panning and zooming keeps the field locked to the islands, with no drift.
4. No colour literals introduced; the grid reads `--wf-grid-*`.

## Out of scope
- Island layout, clustering, or the controls above the canvas.
