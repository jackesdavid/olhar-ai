# Draw Text — inside/outside bbox placement — Implementation Plan

**Board card:** Draw Text: inside/outside bbox placement (ToDo)
**Goal:** Add a parameter to the **Draw Text** node letting the user choose whether the label is
drawn **inside** the bounding box (current behaviour) or **outside** it — e.g. the class label
sitting just *above* the box top edge, like a classic detection overlay ("hall" tag over the person).

## Context (grounded in the code)

The **Draw Text** node already exists — it is a distinct node, not part of Draw BBox:

- **Kind**: `NodeKind::DrawText` (`crates/app-core/src/workflows/domain.rs:379`).
- **Ports**: `DRAW_TEXT_IN` = `image*` (Image), `bbox*` (Bbox), `text` (Text, optional wire) →
  `DRAW_TEXT_OUT` = `image`, `ok` (`domain.rs:688`).
- **Default config** (`domain.rs:1352`):
  `{ "text": "", "position": "top-left", "size": "18", "text_color": "#FFFFFF", "background": true, "bg_color": "#000000" }`
- **Execution**: `run_draw_text` in `crates/app-core/src/workflows/node/vision.rs:240` — renders with
  `ab_glyph` + `imageproc::drawing::{draw_filled_rect_mut, draw_text_mut, text_size}`.
- **Property panel**: the `position` dropdown is a per-kind `{:else if key === "position" && selectedNode.kind === "draw_text"}`
  block in `sentinel-app/src/routes/workflows/[[id]]/+page.svelte:3744`.

Today `position` picks one of 4 corners (`top-left`, `top-right`, `bottom-left`, `bottom-right`)
and the label box is **always anchored inside** the bbox with `pad = 4` (`vision.rs:284-289`):

```rust
let (tx, ty) = match position {
    "top-right"    => (b.x2 - tw - pad, b.y1 + pad),
    "bottom-left"  => (b.x1 + pad,      b.y2 - th - pad),
    "bottom-right" => (b.x2 - tw - pad, b.y2 - th - pad),
    _              => (b.x1 + pad,      b.y1 + pad), // top-left
};
```

Then both coords are clamped to keep the label on-screen (`vision.rs:291-292`).

## The change — a new `placement` parameter

Add a `placement` config field with values `inside` (default, current behaviour) and `outside`.
`placement` only flips the **vertical** anchor relative to the box edge; the horizontal edge
(left/right) still comes from `position`. So a `top-*` label with `placement: outside` sits
**above** the box; a `bottom-*` label sits **below** it.

## Phase 1 — Rust node (`crates/app-core`)

1. **Default config** — `domain.rs:1352`: add `"placement": "inside"` to the `DrawText` JSON so
   existing graphs keep the current look:
   ```rust
   NodeKind::DrawText => json!({ "text": "", "position": "top-left", "placement": "inside",
       "size": "18", "text_color": "#FFFFFF", "background": true, "bg_color": "#000000" }),
   ```
2. **Execution** — `vision.rs:run_draw_text` (~line 267): read the new field
   `let placement = config.get("placement").and_then(|v| v.as_str()).unwrap_or("inside");`
   Then split the anchor into horizontal (from `position`) and vertical (from `position` +
   `placement`). The label box spans height `th + pad*2`, so for `outside`:
   - top edge (`top-*`): draw the whole label box **above** `b.y1` → `ty = b.y1 - th - pad` (its
     background rect top becomes `ty - pad = b.y1 - th - 2*pad`).
   - bottom edge (`bottom-*`): draw **below** `b.y2` → `ty = b.y2 + pad`.
   Keep the existing horizontal `tx` (`b.x1 + pad` for left, `b.x2 - tw - pad` for right) and the
   existing on-screen clamp (`vision.rs:291-292`) untouched — it already prevents the label going
   off-frame when the box hugs the top edge.
3. **No port/schema changes** — `placement` is config-only, so `DRAW_TEXT_IN/OUT`, `node.rs`
   dispatch (`node.rs:427`) and `node_timeout` need no edits.

## Phase 2 — Frontend property panel

- `sentinel-app/src/routes/workflows/[[id]]/+page.svelte`: add a `placement` select next to the
  existing `position` block (~line 3744), mirroring the `style`/`position` dropdown pattern
  (`Select.Root` with a `[{value,label}]` list, default `"inside"`):
  ```
  Inside box · Outside box
  ```
  Config keys render generically by iterating `selectedNode.config`, so a new key auto-appears;
  the only work is the dedicated `{:else if key === "placement" && selectedNode.kind === "draw_text"}`
  branch so it's a dropdown, not a plain text `Input`.

## Phase 3 — Docs + deploy

- `docs/workflow-nodes.md`: update the **Draw Text** row's config column to list
  `placement (inside/outside, default inside)`.
- `task cargo` (workspace check) + `task check` (svelte-check).
- Commit + `task cloud-deploy -- sentinel-flow` (node execution runs in flow), then redeploy the
  web app (frontend) per the standard deploy path.

## Open decisions (grill before building)

1. **Param name** — `placement` (inside/outside) reads clearly and stays orthogonal to `position`.
   Alternative: fold it into `position` as 8 values (`top-left-outside`, …) — rejected: doubles the
   dropdown and churns existing configs. Recommend `placement`.
2. **Outside semantics for left/right-only intent** — proposal keeps horizontal edge from
   `position` and only flips the vertical side. A `top-left` + `outside` label therefore appears
   above the box, left-aligned to it (matches the reference screenshot). Confirm that's the wanted
   behaviour vs. e.g. left-of-box placement (not proposed).
3. **Clamp vs. overflow when the box touches the frame top** — reuse the existing clamp so an
   `outside`/top label that would go off-frame is nudged back on-screen (it may then overlap the box
   top). Acceptable for v1; a "flip to inside when no room" heuristic is out of scope.

## Out of scope (v1)

- Left/right *outside* placement (label beside the box).
- Auto-flip inside↔outside based on available room.
- Connector line/leader from label to box.
