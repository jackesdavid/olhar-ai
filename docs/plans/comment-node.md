# Comment Node — Implementation Plan

**Board card:** Comment node — canvas organizer (ToDo)
**Goal:** A purely visual, resizable group-box node for annotating the workflow canvas, like Unreal Engine's Comment node.

## Context

The graph is stored as a single JSONB blob (`workflows.data`) — no separate node columns, so no migration is needed. Extra fields (`width`, `height`) go straight into the node's `config` JSON. The canvas is SVG-based; z-order is DOM order, so comments render behind by splitting the node loop into two passes.

## Architecture decision: backend-registered, engine-skipped

Comment is added to `NodeKind` (so it round-trips through the graph serializer cleanly), but the engine skips it immediately — zero overhead. It has no ports and never runs. The alternative (frontend-only metadata outside `nodes[]`) would require a custom API shape; reusing the node mechanism is free.

## Phase 1 — Backend (app-core)

- `crates/app-core/src/workflows/domain.rs`:
  - Add `Comment` variant to `NodeKind` enum
  - Add to `ALL` array (bump count)
  - `slug` → `"comment"`, `label` → `"Comment"`, `category` → `"Utility"`
  - `input_ports` → `NO_PORTS`, `output_ports` → `NO_PORTS`
  - `default_config` → `json!({ "text": "Comment", "color": "#facc15", "width": 400, "height": 200 })`
- `crates/app-core/src/workflows/engine.rs`:
  - Add early `continue` for `NodeKind::Comment` (before the run pipeline — same pattern as FlipFlop/Cooldown engine-driven block, but even simpler: just skip)
- `crates/app-core/src/workflows/node.rs`:
  - Dispatch arm: `NodeKind::Comment => Ok(PortValues::new())`
  - `node_timeout` `None` arm: add `| NodeKind::Comment`
  - `node_log_summary` arm: `NodeKind::Comment => "comment".into()`

## Phase 2 — Frontend (`sentinel-app/src/routes/workflows/+page.svelte`)

### 2a — Type extension

Extend the local `Node` type to carry optional `width`/`height` read from config:

```typescript
// helpers to read comment size from config
function commentW(n: Node): number { return Number(n.config.width) || 400; }
function commentH(n: Node): number { return Number(n.config.height) || 200; }
```

### 2b — Rendering (z-order split)

Split the single `{#each graph.nodes}` into two passes inside the `<g transform>`:

```
<!-- Pass 1: comment nodes (behind everything) -->
{#each graph.nodes.filter(n => n.kind === 'comment') as n}
  <!-- translucent colored rect + top label bar + resize grip -->
{/each}

<!-- Pass 2: edges (unchanged) -->

<!-- Pass 3: regular nodes (unchanged, on top) -->
{#each graph.nodes.filter(n => n.kind !== 'comment') as n}
```

**Comment node SVG shape:**
```
<g transform="translate({n.x} {n.y})" onpointerdown={e => startDrag(e, n)}>
  <!-- background fill -->
  <rect width={commentW(n)} height={commentH(n)} rx="10"
        fill={n.config.color} fill-opacity="0.15"
        stroke={n.config.color} stroke-opacity="0.5" stroke-width="1.5" />
  <!-- header bar with text label -->
  <rect width={commentW(n)} height={28} rx="10"
        fill={n.config.color} fill-opacity="0.35" />
  <text x="10" y="18" font-size="13" fill="currentColor" class="select-none">
    {n.config.text || 'Comment'}
  </text>
  <!-- resize grip triangle (bottom-right corner) -->
  <polygon points="{commentW(n)-16},{commentH(n)} {commentW(n)},{commentH(n)-16} {commentW(n)},{commentH(n)}"
           fill={n.config.color} fill-opacity="0.6"
           class="cursor-se-resize"
           onpointerdown|stopPropagation={e => startResize(e, n)} />
</g>
```

### 2c — Resize state + handlers

Add alongside the existing `drag` state:

```typescript
let resize: { id: number; startX: number; startY: number; startW: number; startH: number } | null = null;

function startResize(e: PointerEvent, n: Node) {
  if (editLocked) return;
  e.stopPropagation();
  resize = { id: n.id, startX: e.clientX, startY: e.clientY,
             startW: commentW(n), startH: commentH(n) };
  svgEl?.setPointerCapture?.(e.pointerId);
}
```

In `onCanvasMove`: when `resize` is set, update `config.width`/`config.height` reactively:
```typescript
if (resize && graph) {
  const dx = (e.clientX - resize.startX) / zoom;
  const dy = (e.clientY - resize.startY) / zoom;
  const n = graph.nodes.find(x => x.id === resize!.id);
  if (n) {
    n.config = { ...n.config,
      width: Math.max(120, Math.round(resize.startW + dx)),
      height: Math.max(60, Math.round(resize.startH + dy)) };
  }
}
```

In `onCanvasUp`: persist via `setNodeConfig` and clear `resize`.

### 2d — Inspector panel

The comment node's inspector (right panel, shown when selected) shows only:
- **Text** — plain `<Input>` (single line), saved via `saveNodeConfig`
- **Color** — `<input type="color">` mapped to the `color` config field

Skip the "Memoize" toggle, port list, and "Run / Active" badge — those are meaningless for a comment.

### 2e — Node palette

Comments should appear in the palette under "Utility" (auto-derived from `NodeKind::ALL`). No special case needed.

### 2f — "Drag to move contained nodes" (deferred)

When dragging a comment, optionally translate all nodes whose center falls within the comment's rect. This is the killer UE feature but adds complexity (need to snapshot positions on drag start). Defer to v2 — for v1 the user moves enclosed nodes manually.

## Phase 3 — Docs

Add to `docs/workflow-nodes.md` Utility section:
```
| **Comment** | — | — | `text`, `color`, `width`, `height` | Visual group box for annotating the canvas. Drag to reposition; drag the bottom-right grip to resize. No execution, no ports. | *(canvas organizer — not part of execution flow)* |
```

## Phase 4 — Deploy

- `cargo check -p app-core` (exhaustive match check)
- Commit + `task cloud-deploy -- sentinel-flow sentinel-control`

## Out of scope (v1)

- Drag-to-move-contained-nodes
- Multi-line text (textarea)
- Locking nodes inside a comment (prevent accidental moves)
- Collapsible/minimize comment to just the header bar
