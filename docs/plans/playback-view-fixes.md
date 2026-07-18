# Playback view — fixes & polish (clip-bar width, zoom, event lanes, layout)

**Board card:** #116 Playback view fixes — clip-bar width, granular zoom, per-kind lanes, full-height (ToDo)
**Follows:** #67 Playback Timeline (Done) — this is a fix/polish pass on that view.
**Source:** owner report + screenshots, 2026-07-18.
**Files:** `sentinel-app/src/routes/playback/+page.svelte` (798 lines) + `sentinel-app/src/lib/timeline.ts`.
**Goal:** The timeline reads truthfully (bars match footage), zooms to the minute with a visible,
persisted level, opens on the newest footage, distinguishes "now", lanes events by kind, and uses the
full viewport height without clipping.

## Phase 1 — Clip bar honours real footage (the bug)

**Symptom:** the playhead sits exactly at a clip's end, yet the clip/session bar extends further
right ("more data ahead"). The bar's length doesn't match the footage.

**Where:** `segWidth` (`+page.svelte:122`) = `widthOf((s.endTs - s.startTs)/1000, pxPerSec)` over a
**merged session** (`mergeSegments`, `timeline.ts:105`, `gapToleranceSecs = 20`). Candidate causes,
to confirm against real rows in the implement pass:
1. **Merge tolerance folds a trailing gap into the bar** — a 20s inter-clip gap is drawn as footage,
   so a session bar can extend ~20s past the last real frame.
2. **`widthOf`'s `Math.max(2, …)` min-width** (`timeline.ts:56`) inflates a short clip at coarse zoom.
3. **`duration_secs` > actual frames** (recording metadata padding / pre-roll).
- **Fix:** clamp the rendered bar to actual footage — the session bar ends at the last clip's true
  end (no gap padding), and reconsider the 2px floor (or cap it so it can't exceed the real span).
  Verify the bar's right edge lands on the playhead when the playhead is at the clip end.

## Phase 2 — Granular, visible, persisted zoom

- **More levels, to the minute.** `PX_PER_SEC_PRESETS = [0.02,0.05,0.1,0.25,0.5]` (`timeline.ts:146`)
  tops out at 0.5 px/s (1 min = 30px). Add finer presets (e.g. `…,1,2,4`) and intermediate steps so
  zooming resolves minute-by-minute; `rulerTicks` already has a 60s candidate, so 1-min ticks appear
  once `pxPerSec ≥ minPx/60 ≈ 1.2`.
- **Show the level on screen.** Between the zoom buttons (`+page.svelte:627-639`) render the current
  level — the ruler step (e.g. "1 min", "5 min") or px/s — so the user sees where they are.
- **Persist across refresh.** Save `pxIdx` (and reasonably the `day`/`activeCamera`) to
  `localStorage` (web-only app; no Tauri prompt), restore on mount. `zoom()` (`:203`) sets `pxIdx`.

## Phase 3 — Open on the newest footage

`load()` (`+page.svelte:180`) sets `playheadMs = Math.min(...starts)` — the **earliest** clip. Change
to the clip **closest to now** — the latest clip whose start ≤ `nowMs` (else the last clip of the day)
— then `revealPlayhead()` scrolls it into view. So a refresh lands on the most recent footage.

## Phase 4 — "Now" reads distinct from the playhead

The now-guide (`+page.svelte:754`) is `bg-primary/40`; the playhead (`:759`) is `bg-primary`. They're
the same hue at different alpha — easy to confuse. Give "now" a distinct treatment: a **semantic
accent** (amber, the theme's warn hue — colour communicates state, not brand, per the design system)
and/or a small "now" tick label, so current-time ≠ scrubber.

## Phase 5 — One lane per event kind (the explorer)

Today all events fold into a **single** marker lane clustered by time+severity (`buildMarkers`,
`timeline.ts:174`; rendered `+page.svelte:704-725`). Owner wants **each event kind on its own lane**.
- Group `shownEvents` by kind (`event`/`kind` field on `EventRow` — motion, recording, presence,
  clip, auth, …), one lane per kind, with a left-column label (mirror the camera track headers,
  `:646-676`) and the lane's markers/dots along the row.
- Keep clustering **within** a kind's lane (reuse `buildMarkers` per kind). Decide lane ordering
  (by a fixed kind order / by frequency) and how many kinds to show (all vs top-N + "other").

## Phase 6 — Full-height, no clipping

Screenshot: the timeline card occupies only the top strip; the event popover is clipped and the lower
viewport is empty. The scroll content (`+page.svelte:679`) is `overflow-x-auto` only — tall content
(many tracks + per-kind lanes) is cut off.
- Make the view fill the viewport: verify the `h-full`/`flex min-h-0 flex-1` chain from the page
  container (`:495`) → content (`:546`) → timeline (`:644`) actually resolves to full height (the
  parent route/layout may not be `h-full`).
- Give the scrolling body **`overflow-y-auto`** (with `overflow-x-auto`) so tracks + the new per-kind
  lanes scroll vertically instead of clipping; keep the ruler/marker header sticky if it reads better.
- Ensure the hover/detail popover (`:772`) isn't clipped by an `overflow-hidden` ancestor.

## Open decisions (grill before building)

1. **Clip-bar root cause** — confirm which of the 3 candidates (merge gap / min-2px / duration_secs)
   actually overshoots, against a real recording; the fix differs per cause.
2. **"Now" colour** — amber (warn hue) vs a neutral distinct style (dotted white)? Monochrome theme
   forbids a brand hue but allows semantic state colour.
3. **Event-lane set** — one lane per *kind* (could be many); cap/scroll vs show-all, and the ordering.
4. **Zoom persistence scope** — just zoom, or also day + active camera + scroll position?

## Out of scope (v1)

- New playback data/sources; server changes (this is all the Playback UI).
- Multi-day timelines; per-clip thumbnails.
