# Node Run Animation: light on arrival, not in a queue — Plan

**Board card:** Node run animation lags — light on arrival, keep min duration (ToDo)
**Goal:** Each node's run highlight should fire the moment its `workflow-node` event arrives (so the animation tracks the logs), while still staying lit for at least a minimum duration so the flow reads. Today highlights are **serialized in a queue**, so the view drifts behind the actual execution.

## Root cause

The highlight controller (`sentinel-app/src/routes/workflows/+page.svelte:294-331`) keeps **exactly one** node live at a time with a single timer + a backlog queue:
- `markLive(id)` (`:303`): if a timer is running, it **pushes to `liveQueue`** instead of lighting now.
- `showLive(id)` (`:310`): sets the single `liveNode` + one `liveTimer` for `MIN_HIGHLIGHT_MS` (700ms).
- `onMinElapsed` (`:314`): only after the timer fires does the **next queued** node light.

So with fast nodes, the Nth node lights ~`N × 700ms` after it actually ran — the visual lag the logs don't show. The minimum-duration hold (intentional, good) is correct; the **serialization** is the bug.

## Fix — concurrent, per-node timed highlight

Replace the single `liveNode`/`liveQueue`/`liveTimer` with a **set of live nodes**, each with its own expiry:
- State: `liveNodes = $state<Set<number>>(new Set())` (or `Map<number, timerHandle>`).
- `markLive(id)`: **add `id` immediately** (light it now), and (re)arm a per-node timer for `MIN_HIGHLIGHT_MS`; on expiry remove `id` from the set — **unless** it's still the run's current node and the run hasn't ended (keep the actively-running node lit until the next event, preserving today's "current stays lit" feel).
- No queue, no waiting: many nodes can be lit at once; each appears on arrival and lingers ≥ min duration. Re-marking a node already lit just extends its expiry.
- `endLive()`: mark run ended; let each node's own timer drain its minimum, then clear (don't hard-clear instantly).
- `clearLive()`: cancel all per-node timers + empty the set (graph switch / reset).
- `nodeLive(id)` (`:404`): `return liveNodes.has(id)` — drives the node + edge highlight (`:522`, `:1626`, `:1648`).
- The `act.current` re-light fallback (`:413-415`, when navigating to the editor mid-run) keeps working: if the current node isn't already lit, `markLive(cur)`.

## Out of scope
- Changing `MIN_HIGHLIGHT_MS` (keep the 700ms minimum — it's the intentional "feel").
- The marching-ants / glow CSS (`app.css` `node-march`/`node-run-glow`) — visuals unchanged; only *when* a node is considered live changes.

Developer:
"Can we do a gently fade in fade out animation to create a more cool transitions in the moments we re-trigger the animation again ? that will happen in branchs with loops or events that can be trigged many times... just a ideia"
## Verification
- Run a fast multi-node graph with the console open: node highlights now track the console timestamps (no growing lag); several nodes can pulse near-simultaneously on a burst, each still visibly held ≥700ms.
- A slow node stays lit until the next fires; on run end, highlights drain their minimum then stop. Switching graphs clears them.
