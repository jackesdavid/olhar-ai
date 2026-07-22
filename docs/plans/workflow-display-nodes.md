# Workflow display nodes — Implementation Plan

**Board card:** #135 Workflow display nodes (Title / Text / Paragraph / Gauge / Markdown / Tabs)
**Goal:** Expand the editor's "show" nodes (today only `ShowImage`) into a small family of
presentation nodes that render content **inside the node on the canvas** — for documenting a graph
and eyeballing live values. No dashboard/grid surface: nodes are positioned freely on the canvas as
today (Level 1 only, per the operator's call).

## Grounding — two existing patterns to extend
- **`ShowImage`** (`+page.svelte:3706-3760`) — the template for drawing runtime content INSIDE a
  node: native SVG (`<image>`/`<text>`/`<rect>`/`<clipPath>`) inside the node's translated `<g>`,
  with `nodeHeight()`/`portsBottom()` grown to fit. Runtime data flows via `repo::set_node_state`
  (persist) + a `ToolUiEvent` on the SSE bus → per-node `$state` cache (`nodeImages`) → prefetch on
  graph open via `nodeStateUrl`. This is the **live-sink** template.
- **`Comment`** (`+page.svelte:3308-3340`) — the template for a config-driven, resizable box:
  `config.{text,color,width,height}`, drawn as SVG, resize gesture (`commentResize`),
  `MANAGED_KEYS.comment=["width","height"]`, no ports, excluded from the node loop
  (`nonCommentNodes`). This is the **static-annotation** template.

## The six nodes, split by family
**Annotation family** (config-driven, no ports, no exec, no backend — pure canvas decoration):
- **Title** — big static text. `config {text, color, size}`. Auto-size.
- **Paragraph** — long multiline text, resizable. `config {text, color, width, height}`.
- **Markdown** — rendered markdown, resizable. `config {text, width, height}`. (Static v1; a live
  wired variant is a later enhancement.)
- **Tabs** — a visual strip of icon+text chips, one "active" chip highlighted; purely decorative
  (no content switching — the operator rejected containers). `config {tabs:[{icon,label}], active, color}`.

**Live-sink family** (ShowImage-like: `exec_in` + a `value` data input, last value persisted +
pushed over SSE; literal config as the unwired fallback):
- **Text** — shows a wired Text value (or the literal), `config {text, bg_color, text_color}`.
- **Gauge** — shows a wired Num as an arc, `config {label, min, max, color, size}`.

## Phase 1 (Slice 1) — Annotation family: Title, Paragraph, Markdown, Tabs
Frontend + `domain.rs` only. No `node.rs`/`engine.rs`/SSE.
- **`crates/app-core/src/workflows/domain.rs`**: add the 4 `NodeKind`s to the enum + `ALL`; `slug`,
  `label`, `category` → new **"Display"** category; ports = `NO_PORTS` (input/output/exec_out) like
  `Comment`; `default_config` per node; `is_pure` untouched (they never execute). Add a `node.rs`
  run arm returning empty `PortValues` only if the `run_node` match is exhaustive (verify).
- **`+page.svelte`**: a `isDisplayNode(kind)` / `isAnnotation(kind)` helper; render each in the
  **foreground node layer** with a custom SVG body (unlike Comment, these are content, not a
  backdrop); no port pins. Reuse the `commentResize` gesture for Paragraph/Markdown (generalize the
  eligibility check); `MANAGED_KEYS` for their `width`/`height`. Markdown: render via a lightweight
  markdown→HTML in a `<foreignObject>` (app is web-only now; note the Safari-transform caveat) or
  styled `<tspan>`s — pick during build. Config UI: extend `configRows` + the field if/else chain
  (reuse color-picker + `<Textarea>` branches; a small list editor for Tabs).
- Icons in the palette icon map; `docs/workflow-nodes.md` + `CLAUDE.md` node count.

## Phase 2 (Slice 2) — Live sinks: Text, Gauge
- **`domain.rs`**: 2 kinds; `input_ports` = `[value]` (Text for Text, Num for Gauge, optional);
  `exec_out` = default (sink passes exec through, like ShowImage); `output_ports` = `NO_PORTS`.
- **`node.rs`**: run arms as sinks (return empty), mirroring `ShowImage`.
- **`engine.rs`**: after the `ShowImage` block (~:1010), a block for Text/Gauge — resolve the
  `value` input, `repo::set_node_state(... {value})`, and emit a new `ToolUiEvent::ShowValue
  {graph, node, kind, value}`.
- **`crates/app-core/src/agent/tools.rs`**: add `ToolUiEvent::ShowValue`. (Node catalog is
  auto-derived from `NodeKind::ALL`; no agent-tools catalog change.)
- **`sentinel-flow/src/events.rs`**: relay `ShowValue` → SSE `workflow-value`.
- **`sentinel-control/src/domain/agent.rs`**: handle `ShowValue` in the agent relay (return `None`).
- **`+page.svelte`**: `nodeValues` `$state` record (like `nodeImages`); `onEvent("workflow-value")`
  handler; prefetch persisted `{value}` on graph open via `nodeStateUrl`; render Text (box + value)
  and Gauge (arc + number) in the body, growing via `nodeHeight`/`portsBottom`; config UI.

## Open decisions
1. **Markdown render surface** — `<foreignObject>` (rich, but WebKit/Safari transform caveat the
   codebase avoided in the Tauri era) vs styled `<tspan>`s (SVG-native, limited). Lean foreignObject
   now that the app is web-only; fall back to tspans if Safari misbehaves.
2. **Tabs "active" interactivity** — v1 is a static `config.active` index (decoration). Clicking a
   chip to change the active index is a nice-to-have, not required.

## Out of scope (v1)
- Any dashboard/grid/layout container or published runtime view (there is none; editor-canvas only).
- Live-wired Markdown/Title (static config only for those).
- Charts/tables/logs widgets from the prototype (separate cards if ever wanted).
