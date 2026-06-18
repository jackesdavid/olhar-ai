# Draw BBox Node — Implementation Plan

**Board card:** Draw BBox node (ToDo)
**Goal:** Workflow node that draws a bounding box on an image and outputs the annotated JPEG.

## Context

Two bbox formats exist in the system:
- **`xywh`** — `[x, y, width, height]` from motion detection (`MotionEvent.bbox`, OpenCV `bounding_rect`)
- **`xyxy`** — `[x1, y1, x2, y2]` from person/face detection (`PersonDetection.bbox`, YOLOX output)

The node must accept both via a `format` config so it wires naturally to either source.

## Phase 1 — Rust node in app-core

- `crates/app-core/Cargo.toml`: add `imageproc = { version = "0.25", default-features = false }` (provides `draw_hollow_rect_mut`)
- `crates/app-core/src/workflows/domain.rs`:
  - Add `DrawBbox` to `NodeKind` enum
  - Add to `ALL` array (bump count)
  - `slug` → `"draw_bbox"`, `label` → `"Draw BBox"`, `category` → `"Vision"`
  - Input ports: `image* (Image)`, `bbox* (NumArray)`
  - Output ports: `image (Image)`
  - Default config: `{ "color": "#00FF00", "thickness": "2", "format": "xyxy" }`
- `crates/app-core/src/workflows/node.rs`:
  - Implement `run_draw_bbox(inputs, config)`:
    1. Decode `image` input (`PortValue::Image`) via `image::load_from_memory`
    2. Read `bbox` as `PortValue::List` → extract 4 `Num` values
    3. Apply `format` config: if `xywh`, convert to `x1,y1,x2,y2` = `x, y, x+w, y+h`
    4. Parse `color` hex string (`#RRGGBB`) → `image::Rgb([r, g, b])`
    5. Parse `thickness` (default 2); draw N concentric hollow rects via `imageproc::drawing::draw_hollow_rect_mut`
    6. Re-encode as JPEG via existing `to_jpeg()` helper → return `PortValue::Image`
  - Add arm to `node_log_summary`: `"bbox drawn on {w}×{h} image"`
  - Add `NodeKind::DrawBbox` to `node_timeout` `None` arm (fast, no watchdog)
  - Dispatch arm in `run_node` match: `NodeKind::DrawBbox => run_draw_bbox(inputs, config)`

## Phase 2 — Docs

- `docs/workflow-nodes.md`: add row to Vision section:
  `| **Draw BBox** | image* (Image), bbox* (NumArray) | image (Image) | color (#RRGGBB, default #00FF00), thickness (px, default 2), format (xyxy/xywh, default xyxy) | Draws a bounding box rectangle on the image at the given coords. | Person detection → Break person detection → (bbox) Draw BBox(color=#FF0000) → Show image |`

## Phase 3 — Deploy

- `cargo check` workspace
- Commit + `task cloud-deploy -- sentinel-flow`

## Open decisions

1. **Label text overlay**: should the node optionally render a text label (confidence, class name) above the bbox? Defer to v2 — requires a font bundle (`rusttype`/`imageproc` text support).
2. **Multiple bboxes**: current design takes a single `bbox (NumArray)`. For drawing all detections from a `For each` loop this is the right granularity. No array-of-arrays input needed.
3. **Color picker in inspector**: the `color` config field renders as a plain text input (hex string). A dedicated color-picker input type in the workflow inspector would be a nice UX improvement — out of scope here.

## Out of scope (v1)

- Text label / confidence overlay
- Filled rect (solid) variant
- Multiple bboxes in one call (use For each → Draw BBox per item)
- Alpha/opacity support
