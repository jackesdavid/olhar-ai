# Recording / Playback Timeline — Implementation Plan

**Use case:** inline (Resolve-style multi-camera playback timeline) — see the mockup captured on the board card.
**Board card:** Playback Timeline — unified inspection view (ToDo, #67)
**Goal:** A DaVinci-Resolve-style playback surface — **preview + camera grid on top, a docked
timeline below** — where every camera is a stacked track on one shared horizontal time axis, the
event stream (#66) rides a shared marker lane above the tracks, and a single playhead scrubs all
of them together. This is the **"time-travel" Live module** the agnostic-event-module plan
([[agnostic-event-module]] Phase 5) explicitly deferred as *"a separate plan"* — the event
substrate now exists; this builds the UI that consumes it.

## North star — the apex inspection surface

> This is the **top point of the whole app** — the single place where *everything* about the
> system is inspected against one playback clock. Not just recordings: **every event from every
> module, laid out along time, correlated with the video it happened in.**

The full journey this screen is walking toward:

1. **Everything in one place.** The only screen that shows all events + all cross-module
   information together. Events ride the timeline at the exact playback moment they occurred; a
   marker **expands** to full detail (the event's `data`, linked clip, room, source module).
2. **Multiple cameras in perfect sync.** Play N cameras at once, all locked to one master
   wall-clock; jump ahead / into the past and *every* active camera moves together.
3. **Detections on the viewport.** Bounding boxes drawn over the playing video, frame-accurate.
4. **Trackers moving along playback (future).** Animated object trajectories that move smoothly
   as the video plays — a replay of the edge `MultiTracker` output.

**Status: BLOCKED on #66 (agnostic event module).** By design — this view consumes the finished
event stream as its correlation backbone. Build starts once #66 lands. Everything below is
research + architecture so we hit the ground running.

## The core mechanic (preserve even if the visual style changes)

> **ruler → shared event-marker lane → N stacked camera tracks, all sharing one horizontal time
> axis, with a single playhead cutting through all of them.**

- **One track per camera**, stacked, with a header column (name, recording mode, clip count,
  hide/solo).
- **Clips** are blocks per camera; **gaps** (nothing recorded) render as dashed lanes.
- **A shared marker lane** under the ruler: each event is one marker at one x (time); hovering it
  lists every camera that had something at that exact moment — *"same moment = same place."*
- **Faint vertical spine lines** drop from each marker through all tracks, so it's obvious which
  clips line up with which event.
- **A single playhead** spans ruler + marker lane + all tracks.
- **Marker styling distinguishes event types** (motion / person / behaviour alert) — feeds the
  ST-GCN behaviour-alert pipeline being built for this codebase.

## What already exists (grounding — reuse, don't rebuild)

| Piece | Where | Use |
|---|---|---|
| Clip metadata + list | `recordings.list({camera_ip,from,to,sort,dir,limit,offset})` → `Recording {clip_id, camera_ip, camera_name, friendly_name, started_at, duration_secs, size_bytes, upload_state}` (`sentinel-app/src/lib/api.ts:222`) | Turn each clip into a track block; `started_at + duration_secs` place & size it; the span between consecutive clips is a gap. |
| Clip playback URL | `recordings.cloudUrl(id)` (presigned R2, 409 if not uploaded) + `recordings.download(id)` (edge blob) (`api.ts:236`) | `<video>.src` in the preview; pick R2 when `upload_state==='uploaded'`, else the edge blob. |
| Camera roster | `recordings.cameras()` → `{ip,name}[]` | The set of tracks + the camera grid tiles. |
| **Event stream (#66)** | `events.list({kind,camera,from,to,after,limit})` → `EventRow {occurred_at, camera, clip_id, kind, severity, room_name, data, title}` (`api.ts:329`; control `domain/events.rs`; core `crates/app-core/src/events.rs`) | The marker lane. `occurred_at` = x; `camera` = which track; `kind`/`severity` = marker style; bucket by rounded `occurred_at` for the "same moment" cluster. |
| Live MSE playback | `/live` (`sentinel-app/src/routes/live/+page.svelte`) — MSE-over-WS via `GATEWAY_BASE`, `streamName(ip)`, HD toggle | Lift the MSE plumbing for the live tail (Phase 5) and the preview player pattern. |
| Recordings library | `/recordings` (`+page.svelte`, 735 lines — card #22) | Sibling surface; this is the *timeline* counterpart to that *table*. Cross-link both ways. |
| Shell + design system | `PageHeader`, `IconChip`, `Loader`/`Busy`, `EmptyState`, `SentinelMark`, tokens in `styles/sentinel-tokens.css` | Reuse; see the Impeccable direction below. |

Nav (`sentinel-app/src/lib/nav.ts`) already has `Live` (`/live`), `Events` (`/events`),
`Recordings` (`/recordings`). This adds one more surface (see Open decision 1).

## Impeccable design direction — reconcile the mockup with the ink theme

The captured mockup is a dark NLE with a **rainbow palette** (teal/amber/violet/rose cameras;
yellow/blue/red event flags). That collides with Sentinel's identity: **monochrome ink
(Clean White / Sentinel Noir), no brand hue, emerald retired, branding carried by form not
colour** ([[monochrome-rebrand]], `.claude/skills/impeccable`). Refinements the build must honour:

- **Cameras are NOT keyed by saturated hue.** Distinguish tracks by the **header label + a small
  ink dot + a subtle per-row ink tint** (`bg-primary` at stepped low opacities / zebra), not
  teal/amber/violet/rose. Clip blocks are **ink** (`--primary` ramp), lifted with `.interactive`.
  (The node editor / 3D map keep colour as their *focal* identity — this surface does not; don't
  borrow their categorical palettes.)
- **Event markers use STATE colour only, never brand colour.** Severity is the language of the
  events feed: `info` → ink dot, `warn` → amber (`--warning`), `error`/alert → red
  (`--destructive`). The *kind* (motion / person / behaviour) shows in the hover tooltip + a small
  glyph, not as a third brand hue.
- **Dots over flags.** The dot-field atom replaces the mockup's diamond flags — markers are
  **circles** (radial falloff), and the live "now" edge is the signature **`.status-live` ping**
  (concentric rings), not a colour swatch.
- **Playhead** is **ink `--primary`** with a small pulse head (on-brand), *not* a red bar — red is
  reserved for error markers so it keeps meaning. (Open decision 2 — a red playhead is the
  universal NLE convention; decide before building.)
- **States:** `Loader` (dot-field) while clips/events load; `EmptyState` (SentinelMark + one line +
  "Record a clip" / "Go live") when a day has no footage; recoverable error toast on fetch failure.
- **Chrome:** `PageHeader` with the `Clapperboard`/`Video` icon-chip; ruler ticks + numbers in
  `tabular-nums`; zoom/scrub controls as `.interactive` chips; 150–250ms ease-out motion, respect
  `prefers-reduced-motion`.

## Phase 1 — Static multi-track timeline (ships value alone)

A read-only day-view of all recordings on one shared axis. No playback yet.

- **Route + shell.** New `sentinel-app/src/routes/playback/+page.svelte` (name TBD — Open
  decision 1). `PageHeader`, a **day picker** (default today), a **camera filter**, a horizontal
  **zoom** control (px-per-second presets).
- **Data.** On day/zoom change, `recordings.list({from: <day 00:00>, to: <day 24:00 / now>})`;
  group items by `camera_ip`; order tracks by `cameras()`.
- **Time model.** A single `pxPerSec` + `originTs` (window start). Helper `xOf(ts)` and `tsOf(x)`
  in a small `timeline.ts` — pure functions, unit-testable, the one source of the axis math (keep
  markup logic out of the template).
- **Render.** `track-headers` column (fixed) + a horizontally-scrolling `tl-content`: **ruler**
  (ticks/labels), **tracks** (one row per camera). Each clip → a positioned ink block
  (`left=xOf(started_at)`, `width=duration_secs*pxPerSec`) labelled with `friendly_name` + time
  range; **gaps** between consecutive clips → dashed lanes.
- **Header per track:** camera name, recording mode (continuous vs motion — from
  `recording_config` / clip cadence), clip count.

## Phase 2 — Shared event-marker lane

- Fetch `events.list({from,to})` for the same window (optionally `camera` filtered by the visible
  tracks). Skip `auth.*`/debug kinds (Open decision 5).
- **Bucket** events by `occurred_at` rounded to a zoom-dependent granularity → one marker per
  moment; the marker's payload is every event in that bucket (camera + kind + title).
- Render markers in a `marker-lane` between ruler and tracks; **hover tooltip** lists each
  camera/kind at that instant (the "same moment = same place" behaviour). Marker colour by
  severity (ink/amber/red per the Impeccable direction).
- **Spine lines:** one faint vertical guide per marker, dropping through all track rows
  (`pointer-events:none`), so clip↔event alignment is visible at a glance.

## Phase 3 — Playhead + scrubbing + synced playback

- **Preview + camera grid on top** (Resolve split): a `<video>` preview for the active camera and
  a small grid of camera tiles; clicking a tile makes it the active camera.
- **Playhead** across ruler + marker lane + tracks. Click the ruler/track or drag → move playhead;
  clicking a marker jumps the playhead to that moment.
- **Playback resolution.** For (active camera, playhead time) find the clip whose
  `[started_at, started_at+duration]` covers it; set `<video>.src` = `cloudUrl(id)` (or edge blob)
  and `currentTime = playhead - started_at`. Play advances the playhead; crossing a clip boundary
  swaps to the next clip; landing in a gap pauses with a "no footage" affordance.
- **v1 plays only the active camera**; other track rows show a static last-frame/placeholder (see
  Open decision 3 — simultaneous multi-camera decode is expensive).

## Phase 4 — Track controls, zoom, keyboard

- **Hide / Solo** per track header (filter visible rows; solo = only this track).
- **Zoom presets** (e.g. 1 min/px … 1 s/px) + fit-to-day; keep the playhead's timestamp stable
  across zoom.
- **Keyboard (Resolve-muscle-memory):** Space = play/pause, ←/→ = step, `,`/`.` = prev/next
  marker, `J/K/L` = shuttle. Respect focus; document in a help affordance.

## Phase 5 — Live tail / "now" (stretch)

- For **today**, the right edge is *now*; past the last recorded clip the active camera can fall
  through to its **live MSE feed** (lift `/live`'s gateway plumbing). The live edge is marked with
  the `.status-live` dot-field ping. Recording + live become one continuous scrub.

## Phase 6 — Multi-camera synchronized playback (the "all cameras in sync" leap)

Elevates Phase 3 from single-active-camera to **N cameras locked to one master wall-clock**. This
is the hard part; see the *Sync engine* appendix. Shape:

- **One master clock = global wall-clock `T`**, never any single video's `currentTime`. A single
  `requestVideoFrameCallback` (rVFC) loop reads the focused video's `metadata.mediaTime`, maps to
  `T`, and pushes per-camera targets to every other tile.
- **Two-tier drift correction:** small drift → nudge `playbackRate` (rubber-band); large drift
  (seek / clip boundary / backgrounded tab) → hard-set `currentTime`. `timeupdate` is unusable
  (fires ~4Hz, no frame info) — rVFC only.
- **Concurrent-decode budget** (start ~6, configurable): only focused/expanded tiles mount a real
  `<video>`; the rest render as **synced keyframe snapshots** at `T`. This is how it scales past
  the browser's practical ~4–9 simultaneous HW-decode ceiling to many cameras.
- **Jump ahead / into the past** = set `T`, every camera independently resolves `T`→(clip, offset)
  or gap (the Phase 1 interval index) and seeks. All tiles move together.
- Stretch "all at once" mode: a **server-composited mosaic** (one HLS stream, à la Frigate
  Birdseye) — one decode session, intrinsic sync, overlays drawn client-side over known tile rects.

## Phase 7 — Detection overlays on the viewport

- An absolutely-positioned `<canvas>` over each `<video>`, same CSS box. On each rVFC callback,
  take `mediaTime` → wall-clock `T`, look up detections at `T`, clear + draw boxes. Keying off the
  *presented* frame's `mediaTime` (not `currentTime`) keeps boxes frame-aligned.
- **Normalized coords** (0..1 of frame) scaled to the video's *displayed content rect*
  (mind `object-fit` letterboxing; `ResizeObserver` to realign). Class/label + score chip per box,
  in the ink theme.
- Data source: replay of the edge tracker (next phase). v1 can render coarse boxes from
  `presence` events' bbox (`RecordPresence` already carries a typed `PortValue::Bbox`,
  `presence.rs:56`) at their `occurred_at`.

## Phase 8 — Tracker trajectory replay (the "future trackers moving" journey)

The edge **already runs a ByteTrack-style tracker** — `crates/app-core/src/tracking.rs`
(`MultiTracker`, Kalman `Track`, `Detection {class, score, xyxy}`, zone/line-crossing lifecycle →
`object_entered`/`object_exited`). Today only *lifecycle events* are persisted, not the dense
trajectory. To replay trackers moving along the video:

- **Persist tracks as timestamped polylines** (research model (c)): one record per track =
  `{track_id, class, label, points:[{t, x, y, w, h, score}…]}`, sampled ~5–10 fps or on change —
  NOT a per-frame bbox table (too heavy). Keyed by `(camera, wall-clock)`.
- At render time, **interpolate** each track's box corners between the two bracketing samples at
  `T` → smooth motion; draw a **trajectory tail** (timestamped polyline up to `T`) for the "moving
  along playback" trail (mirrors Frigate's `ObjectPath` review overlay).
- New storage (a `tracks`/`detections` time-series or R2 sidecar per clip) + an edge→control push
  of the sampled trajectory alongside the existing clip finalization. Ties into ReID ([[reid-person-reidentification]]).

## Open decisions (grill before building)

1. **Where does it live?** (a) New top-level route `/playback` + nav item; (b) a "Timeline" mode
   toggle inside `/recordings`; (c) the "time-travel" tab of `/live`. The event plan framed it as
   *"a future Live module."* Recommend **(a) a dedicated `/playback` surface** cross-linked from
   both Recordings and Live — it spans both and deserves its own axis.
2. **Playhead colour:** ink `--primary` (on-brand, red kept for error markers) vs. the universal
   **red** NLE convention. Recommend ink-with-pulse; decide explicitly.
3. **Multi-camera playback:** v1 = active camera only (others static). Do we ever want *all* tracks
   decoding in sync (heavy: N simultaneous video elements / MSE sessions)? Likely v2, only for a
   small "solo group."
4. **Time window & paging:** day-at-a-time (00:00→now/24:00) v1, vs. an infinite horizontal scroll
   that lazy-loads clips/events per visible range (v2). Recommend day-at-a-time first.
5. **Which events surface as markers?** All kinds, or a curated set (motion / person / behaviour
   alert, hide `auth.*` + `debug`)? Recommend a default filter of "footage-relevant" kinds with a
   toggle to show all.
6. **Marker bucket granularity** for "same moment": fixed N seconds, or zoom-dependent
   (recommended — a bucket ≈ a few px at the current `pxPerSec`).
7. **Recording-mode label** ("continuous" vs "motion-triggered"): read from the box
   `recording_config`, or infer from clip cadence? Recommend read config, fall back to inference.
8. **Playback container/format** — the biggest infra decision. Current R2 clips are progressive
   MP4. Smooth arbitrary-wall-clock seeking across many short clips wants **fMP4/CMAF over HLS**
   with `PROGRAM-DATE-TIME` (wall-clock → segment lookup) + short GOP (~1–2s). Do we re-mux
   recordings to segmented HLS (affects the edge recorder + [[cloud-recording-storage]]), or ship
   v1 on faststart MP4 and accept coarser seeking? Recommend MP4 for v1, HLS as the scaling path.
9. **Cross-camera clock alignment** — "same moment on all cameras" is only true if record-time
   timestamps are. Cameras have independent clocks; derive wall-clock from **RTCP Sender Reports**
   (RTP PTS → NTP) at record time on the edge, and NTP-discipline cameras. How truthful must sync
   be (target < 1 frame)? Decide whether v1 trusts `started_at` as-is or invests in PTS→wall-clock.
10. **Concurrent-decode budget** — the browser realistically decodes ~4–9 simultaneous 1080p
    H.264 streams before jank; past that Chrome silently drops to CPU decode and janks *all* tiles.
    Set an explicit budget (start 6) with keyframe-snapshot fallback for the rest. Benchmark on the
    target hardware early — the ceiling is device-specific.
11. **Detection/tracker storage** — where do replay trajectories live? A Postgres time-series
    (`tracks`), an R2 JSON sidecar per clip, or projected from events? Recommend R2 sidecar (co-located
    with the clip, cheap, lifecycle-managed) + a light index. Volume model: sampled polylines, not per-frame.

## Out of scope (v1)

- Simultaneous multi-camera decode/playback and audio.
- Clip export/trim/markers-out (this is playback, not an editor).
- Cross-day horizontal scroll and hover-scrub thumbnail sprites.
- Writing events/clips from this view — it is read + play only.

## Testing / verification

- `timeline.ts` axis math (`xOf`/`tsOf`/bucketing/gap detection) unit-tested pure.
- Manual: a day with continuous + motion clips + several events across cameras → tracks, gaps,
  markers, spine alignment, playhead scrub across a clip boundary and into a gap. `npm run build`
  clean; verify against a sibling screen for consistency.

---

## Appendix — Browser video-engineering research (the sync engine)

Distilled from a targeted research pass; the hard parts of Phases 6–8. Validate the decode ceiling
empirically on target hardware — it is genuinely device-specific.

**Multi-video frame-accurate sync.** `timeupdate` is unusable (~4Hz, audio-clocked, no frame
info). Use **`HTMLVideoElement.requestVideoFrameCallback` (rVFC)** — fires once per composited
frame with `VideoFrameMetadata`; the load-bearing field is **`mediaTime`** (exact PTS of the frame
*on screen*, same axis as `currentTime`). Architecture: one **master** clock = global wall-clock;
a single rVFC loop reads it and pushes targets to **slave** videos; correct drift in two tiers —
`playbackRate` nudge (0.95–1.05) for small drift, hard `currentTime` seek for large. rVFC is
main-thread/best-effort (can be one vsync late) → continuous soft correction, not hardware genlock.

**Decode ceiling & scaling.** Chrome caps ~75 WebMediaPlayers/tab (memory), but the real limit is
**GPU decode sessions** — comfortably ~4–9 simultaneous 1080p30 before jank; beyond, Chrome falls
back to CPU decode and janks everything. Scale via: **keyframe-snapshot tier** for unfocused
cameras (only focused tiles decode real video), and/or a **server-composited mosaic** (Frigate
**Birdseye** = one HLS stream, intrinsic sync, overlays drawn client-side over tile rects).

**Overlays.** Per-video `<canvas>`, driven by rVFC `mediaTime`→wall-clock→detection lookup;
normalized coords scaled to the *displayed content rect* (`object-fit` letterbox aware,
`ResizeObserver`). Interpolate track polylines between samples for smooth boxes; draw timestamped
polyline tails for trajectories.

**WebCodecs vs `<video>`.** `<video>`+HLS/MSE = cheapest, HW-accelerated, feeds rVFC — ship v1 on
it. **WebCodecs `VideoDecoder`** (demux via mp4box.js in a Worker; `VideoFrame.close()` discipline)
wins only for frame-exact scrubbing or compositing many tiles into one canvas (dodging per-element
decode overhead) — reserve for a v2 precision/mosaic mode.

**Format & seeking.** Progressive MP4 needs `+faststart` (moov front) and still seeks poorly across
hours. **fMP4/CMAF over HLS** with short segments + `PROGRAM-DATE-TIME` (wall-clock tags) + short
GOP is the right substrate for "jump to arbitrary wall-clock across many clips"; HLS.js drives MSE.

**Clock alignment.** IP cameras carry arbitrary RTP timebases; map to wall-clock via **RTCP Sender
Reports** (RTP↔NTP) at record time; NTP-discipline cameras; never trust burned-in OSD time. Target
residual skew < 1 frame; sub-frame genlock needs PTP cameras (not our case).

**Detection/tracker replay data model.** Three options: (a) per-frame bbox rows — exact, too
heavy; (b) sampled keyframes + interpolate — the standard; (c) **tracks as timestamped polylines**
`{track_id, label, points:[{t,x,y,w,h,score}]}` — compact, natural for trails, **recommended**.
Prior art: **Frigate** stores event/track-level data + thumbnails (Norfair/Kalman tracker, SQLite,
`ObjectPath.tsx` renders trajectory in review; bakes boxes into *exported* clips server-side; groups
overlapping detections into "review items" = our marker model) — it does **not** keep a dense
per-frame table. **Shinobi** displays sampled events over the playback timeline. Our edge already
has the tracker (`crates/app-core/src/tracking.rs`); the work is *persisting* its sampled
trajectory (model (c)) for replay.

**Key sources:** web.dev rVFC guide + MDN `requestVideoFrameCallback`; Chromium media-dev
WebMediaPlayer-limit thread; W3C WebCodecs explainer + `video-decode-display` sample; mp4box.js;
HLS fMP4 + `PROGRAM-DATE-TIME`; RFC 6051 + `LukasBommes/rtsp-streamsync` (RTCP wall-clock); Frigate
Review/Birdseye/Object-Tracking docs; Shinobi detect docs.
