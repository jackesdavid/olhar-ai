# Map: Presence History + Logs Panel — Implementation Plan

**Goal:** Show all presence alerts/history and the unified platform_logs stream directly in the map viewer — so operators can browse past detections and see the full audit trail without leaving the map.

## Context

The map currently shows animated pulses (`PresenceStore`) seeded from the last N active events via `GET /map/presence?limit=50`. These expire after their `duration_secs`. There is no way to review past events from the map page.

The right-side `SidePanel` (`components/SidePanel.svelte`) already has two icon tabs — **Properties** and **Explorer** — persisted in `localStorage`. Two more tabs slot in naturally: **Presence** (detection history) and **Logs** (platform_logs stream).

The backend endpoint `GET /map/presence?limit=<n>` (control domain `map.rs`, repo `presence::repo::load_recent`) returns up to 500 rows newest-first. Adding an `after=<id>` cursor param enables pagination.

---

## Phase 1 — Backend: cursor pagination on `/map/presence`

**File:** `sentinel-control/src/domain/map.rs`

Add `after: Option<i64>` to the query struct and pass it to `load_recent`.

**File:** `crates/app-core/src/presence/repo.rs`

Add optional `after_id` param to `load_recent`: append `AND id < $3` when set.

This keeps v1 simple (single fetch of 100–200 rows) while making "Load more" possible.

---

## Phase 2 — SidePanel: Presence + Logs tabs

**File:** `sentinel-app/src/routes/map/components/SidePanel.svelte`

- Extend `Tab` type: `"properties" | "explorer" | "presence" | "logs"`
- Add two icon buttons to the tab strip: `Activity` (presence) and `ScrollText` (logs)
- Each tab lazy-loads on first select; manual refresh icon in each header

**New component:** `sentinel-app/src/routes/map/components/PresenceHistory.svelte`

Props: `presence: PresenceStore, onFlash: (p: Presence) => void`

List rows (newest first):
- Colored category dot (`CATEGORY_STYLE[cat].color`)
- Time (relative: "2 min ago")
- Room name + camera (if present)
- Duration badge (e.g. "30 s")

Footer: "Load more" button (cursor = last row's `id`) + manual refresh icon.

**Wire into `SidePanel`:** pass the `PresenceStore` down from `+page.svelte` (already available there).

---

## Phase 2b — SidePanel: Logs tab

**New component:** `sentinel-app/src/routes/map/components/MapLogs.svelte`

Uses the existing `platformLogs.list()` from `$lib/api` — no new API needed.

Default filter: `module=workflow` + `module=presence` (the two modules relevant on the map).
Optionally the user can toggle to show all modules.

List rows (same compact format as `/logs` page):
- Level badge colored by severity
- Module chip
- Time (relative)
- Message (truncated, full on hover/expand)
- Clicking a `presence` log row that has `entity_id` → flash that location on the map

Footer: "Load more" cursor (`after=<id>`) + refresh.

---

## Phase 3 — Map flash on row click

**File:** `sentinel-app/src/lib/map/presence.svelte.ts`

Add `flash(p: Presence): void` — calls the existing private `add()` with `ageMs = 0` and a short fixed TTL (5 s) so the pulse appears on the map regardless of the event's actual `duration_secs`. This reuses all existing pulse rendering.

In `PresenceHistory.svelte`: `onclick={() => onFlash(row)}` on each row.

---

## Phase 4 — data.ts helper update

**File:** `sentinel-app/src/lib/data.ts`

Extend `loadPresence` to accept an optional `after` cursor:
```ts
loadPresence: (limit = 100, after?: number): Promise<Presence[]> =>
  api.get<Presence[]>(`/map/presence?limit=${limit}${after != null ? `&after=${after}` : ""}`),
```

---

## Out of scope (v1)
- Filter by category or room in the presence tab (easy to add later)
- Full log search / filter in the map logs tab (use the /logs page for that)
- Full-page presence analytics / charts
- Marking a presence event as "reviewed"
