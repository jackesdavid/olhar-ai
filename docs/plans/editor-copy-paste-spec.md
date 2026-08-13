# Copy / Paste in the workflow editor — the actual spec

**Board card:** #180 Copy / Paste — paste at the cursor, config only, no wires
**Goal:** ⌘C takes a node's settings; ⌘V drops a new node with a new id **at the cursor**, carrying
the panel values and nothing else — no wires, no id, no original position.

Duplicate (⌘D / `d`) is **out of scope** and gets its own card. It currently routes through the
same code as paste, so decision 1 below has to settle before it can be split cleanly.

## Why this is a rewrite, not a tweak

Copy/paste has been touched four times this week, and every round fixed *how the keystroke is
delivered* — never what the operation should do. The delivery is now settled (native
`copy`/`cut`/`paste` events with a keydown fallback, see `docs/HANDOFF.md` 2026-08-12). What was
never specified is the behaviour, and the current behaviour is wrong on all three counts:

| | Today | Spec |
|---|---|---|
| Where a pasted node lands | original position `+48, +48` | **at the cursor** (`hoverWorld`) |
| Wires between copied nodes | recreated | **not copied at all** |
| What travels | whole node incl. position | **kind + panel config + memoize** |

## The behaviour, precisely

**Precondition.** The graph must not be active. `editLocked` (`isRunning || graph.enabled`) already
gates the mutating half, and `lockedNotice()` says which of the two states it is. **Copy stays
allowed on an armed graph** — it mutates nothing, and copying *out* of a running workflow into
another one is a real use.

**⌘C** — snapshot each selected node as `{ kind, config, memoize }`. No id (the paste mints one),
no `x`/`y` (the cursor decides), no edges. The clipboard payload keeps its current tagged-JSON shape
on the system clipboard (`olharai/workflow-nodes@1`), so cross-graph and cross-tab paste survive;
only the `edges` array goes away.

**⌘V** — for each entry: `nextNodeId(g)`, a `structuredClone` of the config, and a position derived
from `hoverWorld`, centred exactly the way `addNodeKind` centres a palette drop
(`x - NODE_W/2`, `y - HEAD_H/2`) so a paste and an add feel identical. Select what was pasted, and
`markDirty()` — local until Save, like every other canvas edit (#122).

**Config is what the panel holds.** A wired input's value is runtime data and lives on the edge, not
in `config`, so "only what was chosen in the panel" falls out of copying `config` and dropping the
edges. Nothing needs filtering for the common case — but see decision 2 for the keys that are baked
from wiring rather than typed.

## Phase 1 — Copy

- `snapshotSelection()` in `sentinel-app/src/routes/workflows/[[id]]/+page.svelte` currently returns
  `{ nodes, edges }` with ids and coordinates. Reduce it to `{ nodes: [{ kind, config, memoize }] }`.
- Drop the internal-edge filter entirely.
- `copySelection()` keeps its toast (it is the only feedback the operation has ever had) and its
  `navigator.clipboard.writeText` mirror.
- `onWindowCopy` / `onWindowCut` change only in the payload they serialise.

## Phase 2 — Paste at the cursor

- `pasteClipboard()` stops reading `src.x`/`src.y` and takes an anchor instead.
- Anchor = `hoverWorld`, which `onCanvasPointerMove` already maintains (`+page.svelte:3415`).
- **`hoverWorld` is `null` until the pointer has been over the canvas.** Fall back to the viewport
  centre, the same expression the fit-to-view path uses (`+page.svelte:3514`):
  `{ x: (r.width / 2 - pan.x) / zoom, y: (r.height / 2 - pan.y) / zoom }`. Without this, a paste
  driven from the Edit menu or from a keyboard-only session lands at `NaN`.
- Multi-node selections keep their **relative** layout: compute the copied set's top-left at copy
  time, store each node's delta, and lay the cluster out from the anchor. A five-node copy that
  collapses onto one point is not a paste anyone wants.

## Phase 3 — Prove it

`svelte-check` cannot see any of this, so it needs a real graph:

1. Copy a `Set Variable`, paste — new id, same variable + type, at the cursor, unwired.
2. Copy a node with an inline literal (a `Format` template), paste — the literal survives.
3. Copy two wired nodes, paste — **two nodes, zero wires**.
4. Paste twice without moving the mouse — see decision 3.
5. Copy on an armed graph → allowed; paste → refused with the reason.
6. Copy in graph A, paste in graph B.

## Open decisions

1. **Does Duplicate stay routed through paste?** It shares `pasteClipboard()` today. Once paste
   means "at the cursor", duplicate has to decide whether it also means that or keeps a fixed
   offset — and whether it still overwrites the clipboard. **This blocks the duplicate card**, so
   settle it there; Phase 1/2 should leave `duplicateSelection()` working on its current path.
2. **Config keys that are BAKED FROM A WIRE, not chosen.** `dict_make_row.columns` is copied off a
   wired Dictionary Schema; `tuya_control.value_type` is derived from the chosen data point. With
   edges no longer copied, a pasted `dict_make_row` keeps ports whose schema wire is gone.
   Proposed: **keep them.** The ports are the node's shape, and a node that pastes back to a bare
   stub looks broken. Stripping is the reversible choice if it proves wrong.
3. **Two pastes without moving the mouse** stack exactly, which is literal to the spec and unusable.
   Proposed: exact position for the first paste, then a small cascade for repeats while the anchor
   has not moved. Flagged rather than assumed — the spec says "the last mouse position".
4. **Pasting into a graph that lacks the variable.** A `Set Variable` pasted into graph B references
   a declaration that may not exist there. Proposed: paste it anyway and let the node render as
   incomplete (the editor already marks required-unfilled with a red border), rather than silently
   dropping the node or the config.

## Out of scope

- Duplicate — its own card, after decision 1.
- Copying wires. Explicitly not wanted: the pasted node is a fresh node to be wired by hand.
- Any change to how the keystroke arrives. That is settled and must not be reopened as part of this.
