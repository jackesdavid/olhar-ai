# Node input: inline-value indicator — Implementation Plan

**Board card:** Node input: inline-value indicator (ToDo)
**Goal:** On the graph canvas, show at a glance which of a node's **unconnected** input ports
carry a value the user typed in manually (an inline literal in `config`), so you can tell a
configured input from an empty one without opening the property panel.

## Grounding (all in `sentinel-app/src/routes/workflows/[[id]]/+page.svelte`)
The editor is one monolithic SVG route — no separate node component. Everything needed already
exists; we mostly expose a new per-port flag at render time.

- **Manual value storage:** an inline literal for input port `foo` lives at `node.config["foo"]`
  (config key == port name). There is no separate values map. Confirmed both sides: frontend
  reads `node.config[port.name]` (L721); Rust `GraphNode.config` is documented "Static settings +
  literal fallbacks for unconnected input ports" (`crates/app-core/src/workflows/domain.rs:1457–1465`),
  resolved by `literal_for_named()` (`node.rs:202`).
- **Input-port render loop:** `{#each dataPorts(k,"in") …}` at **L3176–3201**. In scope per port:
  `p` (`{name,type,required}`), `wired = wiredInto(n.id, p.name)` (L3178, the Edge or undefined),
  `need = p.required && !isFilled(n,p)` (L3179). The dot `<circle>` + label `<text>` are L3181–3200.
- **"Is it wired?"** — `wiredInto()` (L1021) / `filledPorts` O(1) set (L593).
- **"Has inline value?"** — no standalone helper today. `isFilled(node,port)` (**L719–732**)
  deliberately conflates wired OR non-empty-literal; the per-type "config literal is a usable
  value" checks live at L721–731.
- **Existing per-port visual state to stay consistent with** (L3181–3200): unconnected dot uses
  `--wf-port-empty`; wired dot uses `portColor[p.type]`; required-but-empty draws a
  `--destructive` stroke + red label. Node-level red badge via `requiredUnfilled()` (L735).

## Phase 1 — a `hasInlineValue` predicate
- Extract the "config literal is a usable value" branch out of `isFilled` (L721–731) into a shared
  helper, then add `hasInlineValue(node, port) = !wiredInto(node.id, port.name) && <that branch>`.
  `isFilled` keeps its current meaning by reusing the same extracted check (no behaviour change).

## Phase 2 — render the indicator on the port
- In the input-port loop (L3181–3200), add `{@const inline = !wired && hasInlineValue(n, p)}` and
  give an inline-valued dot a distinct look — a small filled ring / inner dot on the port circle,
  or a filled (vs hollow) `--wf-port-empty` dot. Keep it clearly different from both the wired
  color and the required-red states, and add a `<title>` ("value set manually") for hover.
- Precedence when a port is required-but-empty vs inline-filled: an inline value means it's *not*
  empty, so `need` is already false there — no conflict, but verify the styling ordering.

## Open decisions (grill before building)
1. **Visual treatment.** Options: (a) fill the otherwise-hollow empty dot; (b) a small dot/ring
   badge on the port; (c) tint the port label. Recommend **(a)** — cheapest, reads instantly, and
   reuses the existing empty-vs-typed-color language. Needs an impeccable/design pass since it
   touches the monochrome port language.
2. **Also indicate on the node body** (e.g. a subtle "N configured inputs" marker) or ports only?
   Default: ports only — the ask is per-input.

## Out of scope (v1)
- Showing the *value itself* on the canvas (hover tooltip with the literal could be a follow-up).
- Any backend change — this is purely a canvas-render read of existing `config`.
- Set/Get Variable or exec ports — this is about data inputs with literal fallbacks.
