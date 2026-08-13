# Copy / Paste in the workflow editor — the actual spec

**Board card:** #180 Copy / Paste — paste at the cursor, config only, no wires
**Goal:** ⌘C takes a node's settings; ⌘V drops a new node with a new id **at the cursor**, carrying
the panel values and nothing else — no wires, no id, no original position.

Duplicate (`d`) is **out of scope** and gets its own card. It used to route through paste; this
change stands it on its own so that card is a clean edit — see the decisions below.

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
edges. Nothing needs filtering for the common case — but see the decisions for the keys that are
baked from wiring rather than typed.

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
4. Paste twice without moving the mouse — the second cascades 24px, it does not stack.
5. Copy on an armed graph → allowed; paste → refused with the reason.
6. Copy in graph A, paste in graph B.

## Decisions taken (implemented 2026-08-12)

1. **Duplicate is off the clipboard entirely.** It used to be `clipboard = snapshot;
   pasteClipboard()`, which was fine while paste also meant "offset from the original" — but paste
   now means "at the cursor", and sharing the path would have dragged duplicate along with it.
   `duplicateSelection()` now clones in place at `DUPLICATE_OFFSET`, behaviour unchanged, ready for
   its own card to edit cleanly. Side effect worth having: duplicating no longer throws away what
   you had copied.
2. **Baked-from-wire config keys are KEPT** (`dict_make_row.columns`, `tuya_control.value_type`).
   The ports are the node's shape; a paste that returned a bare stub would look broken. Reversible.
3. **Repeated paste cascades.** First paste lands exactly on the cursor; each repeat at an unmoved
   anchor steps `PASTE_CASCADE` (24px) down-right. Moving the mouse resets it. Stacking exactly was
   literal to the spec and unusable.
4. **A pasted node whose variable does not exist in the target graph is pasted anyway**, rendering
   incomplete (the editor already draws required-unfilled with a red border). No code needed —
   this is what falls out of not filtering.

Also handled, not in the original plan: `parseNodePayload` now **validates rather than trusts** the
pasted text, and coerces a missing `dx`/`dy`. A payload copied before this change carries ids,
absolute coordinates and edges; it still pastes, as a stack at the cursor, instead of throwing.

## Out of scope

- Duplicate — its own card. It is decoupled here but its behaviour is unchanged.
- Copying wires. Explicitly not wanted: the pasted node is a fresh node to be wired by hand.
- Any change to how the keystroke arrives. That is settled and must not be reopened as part of this.
