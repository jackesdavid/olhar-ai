# Node Body: Live State Display — Implementation Plan

**Board card:** Node body: cooldown live state + wait config display
**Goal:** Show meaningful data inside the Cooldown and Wait nodes on the canvas — cooldown
shows live remaining seconds (from persisted node state), wait shows its configured duration
(static from config). Reuse all existing mechanics.

## What we already have

- `workflow_node_state` table — cooldown already writes `{ "last_fired_at": <unix_ms> }`
  into it via `repo::set_node_state()` in `engine.rs`.
- `repo::get_node_state(db, tenant_id, graph_id, node_id)` — reads the JSON blob back.
- ShowImage node pattern: `runFrameUrl` in `data.ts` hits `/flow/frame?...` on sentinel-flow
  which reads from `workflow_node_state`. Frontend fetches on graph open + updates via SSE run events.
- Node body rendering is done inline in `+page.svelte` — the node's `kind` already drives
  per-kind rendering (ShowImage preview, etc.).

## Phase 1 — Flow API: generic node state endpoint

**`sentinel-flow/src/http.rs`**  
Add `GET /flow/node-state?tenant=<uuid>&graph=<id>&node=<id>` alongside the existing `/flow/frame`
handler. Returns the raw `state_json` as JSON (or 404 if no state saved yet).

```rust
async fn node_state(State(state): State<Arc<AppState>>, Query(q): Query<FrameQuery>) -> impl IntoResponse {
    let db = state.db.clone();
    let saved = tokio::task::spawn_blocking(move || {
        app_core::workflows::repo::get_node_state(&db, q.tenant, q.graph, q.node)
    }).await.ok().flatten();
    match saved {
        Some(v) => Json(v).into_response(),
        None     => StatusCode::NOT_FOUND.into_response(),
    }
}
```

Register it: `.route("/flow/node-state", get(node_state))`.

**`sentinel-app/src/lib/data.ts`**  
Add `nodeStateUrl(graph, node)` (same shape as `runFrameUrl`) and a typed fetcher:
```ts
nodeState: (graph: number, node: number) =>
  fetch(`${FLOW_BASE}/flow/node-state?tenant=${tenant}&graph=${graph}&node=${node}`)
    .then(r => r.ok ? r.json() : null)
```

## Phase 2 — Cooldown: live remaining display

**State in `workflow_node_state`:**  `{ "last_fired_at": <unix_ms> }`  
**Derived in frontend:** `remaining = window_secs - Math.floor((Date.now() - last_fired_at) / 1000)`

**`sentinel-app/src/routes/workflows/+page.svelte`**

- Add `let cooldownStates = $state<Record<number, number | null>>({})` — key = node id,
  value = `last_fired_at` ms (null = never fired / ready).
- On `openGraph(id)`: fetch state for every cooldown node in the graph (same pattern as
  ShowImage's `for (const n of showNodes)`).
- After each SSE `run_ok` or `run_failed` event: re-fetch cooldown states for the active graph.
- In the node body render, for `kind === "cooldown"`:
  ```svelte
  {@const lastFired = cooldownStates[n.id]}
  {@const windowSecs = Number(n.config?.window_secs ?? 60)}
  {@const elapsed = lastFired ? Math.floor((Date.now() - lastFired) / 1000) : windowSecs + 1}
  {@const remaining = Math.max(0, windowSecs - elapsed)}
  {@const ready = elapsed >= windowSecs}
  <div class="node-body-badge">
    {#if ready}
      <span class="text-emerald-400">Ready</span>
    {:else}
      <span class="text-amber-400">{remaining}s left</span>
    {/if}
  </div>
  ```
- A countdown ticker: `setInterval` that re-derives `remaining` from cached `lastFired` every
  second while a cooldown node is in the active graph. No polling — just local time math from
  the fetched timestamp.

## Phase 3 — Wait node: static config display

No API needed. The `seconds` value is already in `n.config.seconds`.

In the node body render, for `kind === "wait"`:
```svelte
{@const secs = Number(n.config?.seconds ?? 0)}
<div class="node-body-badge text-muted-foreground">
  {secs}s
</div>
```

## Phase 4 — Shared styling

Add a `.node-body-badge` style or a Tailwind class combo reused by both: centered, monospace,
small, low-contrast — sits between the node title and the port rows. No new component needed;
inline in the existing per-kind blocks.

## Deploy

- `task cloud-deploy -- sentinel-flow` (new endpoint)
- `task cloud-deploy -- sentinel-app` (frontend rendering)
- No migration needed (`workflow_node_state` table already exists)

## Out of scope (v1)

- Live countdown ticker while graph is idle (not after a run) — would need WebSocket/SSE push
  from flow when cooldown state changes; polling every second is not worth it.
- Other nodes that have persisted state (ShowImage already has its own display).
- Cooldown node showing a progress bar (just text for now).
