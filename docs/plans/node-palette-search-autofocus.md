# Node palette — focus search on open — Implementation Plan

**Board card:** Node palette — focus search on open (ToDo)
**Goal:** Every time the add-node palette opens, focus its search input so the user can type
immediately — no click needed.

## Grounding
The add-node palette is a **hand-rolled** `{#if open}` popover, not a shadcn Dialog/Command:
`sentinel-app/src/routes/workflows/components/ContextSearch.svelte`.
- Open prop `open = $bindable(false)` (L20); whole palette mounts/unmounts per open (`{#if open}` L75).
- Search `<Input autofocus bind:value={search} …>` at **L107** (shadcn `Input`, a native `<input>`
  wrapper). Text state `search` at L33.
- Opened from three sites in `+page.svelte`, all set `contextOpen = true`: `n`/`N` key
  (L1802–1808), wire-drop on empty canvas (L2355–2356), Add button `openAddNodeMenu` (L2507–2515).
  Bound via `bind:open={contextOpen}` at L4385–4391.

**Why it's flaky today:** it relies on the raw HTML `autofocus` attribute (L107), which only fires
reliably on first DOM insertion and competes with the container's `animate-in … zoom-in-95` mount
animation (L84) and the keydown that opened it. There's no `bind:this` + `.focus()` / `$effect`
focus handling. So it usually works on first open but is exactly the intermittently-failing pattern.

**Robust pattern already in the repo:** `sentinel-app/src/lib/components/searchable-select.svelte:42–44`
```svelte
$effect(() => { if (open && searchEl) searchEl.focus(); });
```
with `bind:this={searchEl}` on its search input. Reuse this for consistency.

## Phase 1 — Focus via `$effect` on open
- In `ContextSearch.svelte`: add `let searchInputEl = $state<HTMLInputElement | null>(null)` and
  `bind:ref={searchInputEl}` on the `Input` at L107 (shadcn `Input` exposes `ref = $bindable(null)`,
  `input.svelte:13`). Drop the raw `autofocus` (and its `a11y_autofocus` ignore, L106).
- Add near the reactive block: `$effect(() => { if (open && searchInputEl) searchInputEl.focus(); });`.
  Fires on every open since `open` is reactive. No `+page.svelte` change — all three open sites
  already set `contextOpen = true`.
- If focus races the mount animation, wrap the focus in `await tick()` (pattern also present at
  `chat/+page.svelte:97`, `playback/+page.svelte:207`).

## Open decisions (grill before building)
1. **Also fix the sibling `NodePalette.svelte`?** (`components/NodePalette.svelte:114` has the same
   `<Input autofocus>` pattern.) The editor page uses `ContextSearch`, not `NodePalette` — confirm
   whether `NodePalette` is still mounted anywhere; if dead, leave it (or note for cleanup).

## Out of scope (v1)
- Converting the palette to a shadcn Command/Dialog component.
- Any change to search behaviour, ranking, or the open triggers themselves.
