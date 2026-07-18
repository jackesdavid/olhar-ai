# Enabled / Debug / Orphans → top-of-canvas icon toggles — Implementation Plan

**Board card:** Enabled / Debug / Orphans → top-of-canvas icon toggles (ToDo)
**Goal:** Lift the three editor toggles out of the Details-tab panel to a floating cluster at the
**top of the canvas**, rendered as beautiful icon toggles, ordered **Enabled → Debug → Orphans**.

## Grounding (all in `sentinel-app/src/routes/workflows/[[id]]/+page.svelte` — single file, no components/)
Today all three are shadcn `Switch` rows inside the **Details tab** (`{#if panelTab === "details"}`
opens L3561), each `<Label><Icon/> …</Label> + <Switch>`:
- **Enabled** — L3607–3613. `checked={graph.enabled}` → `toggleEnabled` (L1548–1557). Icon `Power`.
- **Orphans** — L3615–3626. `checked={graph.run_orphans ?? false}` `disabled={editLocked}` →
  `toggleRunOrphans` (L1558–1567). Icon `Unplug`. (Source order today: Orphans *before* Debug.)
- **Debug** — L3628–3639. `checked={graph.debug ?? false}` `disabled={editLocked}` → `toggleDebug`
  (L1568–1577). Icon `Bug`.
- `Switch` imported L73; icons are Lucide `Power`/`Unplug`/`Bug` (`size-3.5` today).

**What they do** (unchanged by this move):
- **Enabled** = the graph "armed" bit — arms the trigger. Server-persisted `PUT /workflows/:id/enabled`
  (`data.ts:315`, Rust `set_enabled` `workflows.rs:990`). Drives `editLocked = isRunning || graph.enabled`
  (L654), StatusDot, run title. NB: `set_trigger` force-sets `enabled=false` on any trigger change
  (`workflows.rs:883`) — same bit, relevant once #111 lands.
- **Debug** = per-node debug log line to the editor console (`PUT /workflows/:id/debug`, `data.ts:321`).
- **Orphans** = run behaviour: execute unconnected nodes vs skip them (`PUT /workflows/:id/run-orphans`,
  `data.ts:318`). Gates "can run" (L723) + the "enable orphans" run messages (L770/791).

**Persistence:** all three are **per-graph, server-persisted** (optimistic update + `reload()` +
rollback), bound to `graph.*`. **No localStorage.** Moving the markup is purely presentational —
keep the same `checked`/`onCheckedChange` bindings and `disabled={editLocked}` on Debug/Orphans.

**Floating-cluster house style** (top row: Add-node `left-3 top-3` L3398, transport `left-1/2 top-3`
L3440, zoom `right-3 top-3` L3462; banner `<!-- floating controls -->` L3397). Cluster wrapper:
`absolute … top-3 flex items-center gap-1 rounded-lg border border-border bg-hud p-1 shadow-lg backdrop-blur`.
Icon button: `<Button variant="ghost" size="icon" class="size-8" title="…"><Icon class="size-4" /></Button>`.
Active/on idiom: `class="size-8 {active ? 'bg-primary/15 text-primary' : ''}"` (L3416).

## Phase 1 — Build the top toggle cluster
- Add a new absolutely-positioned sibling `div` in the `<!-- floating controls -->` region using the
  cluster chrome above, holding three icon toggles in order **Enabled (`Power`) → Debug (`Bug`) →
  Orphans (`Unplug`)**.
- Each is an icon `Button` (not a `Switch`) whose on-state uses the `bg-primary/15 text-primary`
  idiom, `title` describing it, calling the **existing** handlers (`toggleEnabled` / `toggleDebug` /
  `toggleRunOrphans`) with `!current`. Reflect `graph.enabled/debug/run_orphans` for the on-state.
- Preserve guards: Debug + Orphans get the `editLocked` disabled treatment (they lock once the graph
  is enabled/running — matches today); Enabled stays interactive. Keep the "off" tooltips explaining
  each (from the current helper text L3620/L3632).

## Phase 2 — Remove the toggles from the Details tab
- Delete the three `<Switch>` rows (L3607–3639) from the Details tab so they live in one place only.
- Keep the **list-view** Enabled switch (L2724) and its StatusDot (L2848) untouched — that's the
  workflows table, not the editor canvas.

## Open decisions (grill before building)
1. **Placement.** Top-center and both top corners are taken. Options: a second-row cluster
   (`top-16`), or tuck it beside the transport cluster. Recommend a distinct top cluster offset from
   the transport — needs an impeccable/design pass since the ask is "beautiful."
2. **Icon-button vs labelled Switch.** The ask says "toggles with icons." Recommend icon `Button`s
   with the active-tint idiom (matches the HUD toolbars) over relocating the `Switch`es. Confirm.
3. **On-state affordance for Enabled.** Enabled is the most consequential (arms the graph). Consider
   a stronger active treatment / the `--status-live` language so "armed" reads unmistakably.

## Out of scope (v1)
- Changing what any toggle does or its persistence/endpoints.
- The workflows list-view Enabled switch.
- Any new toggle — this is Enabled/Debug/Orphans only.
