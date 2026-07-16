# Honest LIVE badge + real loading/error states

**Board card:** #89 Live badge + loading states (ToDo)
**Goal:** The Live tab should tell the truth. Show "LIVE" only when video is *actually playing*, show
*our* loader while connecting, and a clear message when it fails — instead of a green "LIVE" badge
over a permanent black screen, which is what a broken live path looks like today.

Live-camera bug **C**. Sibling: [[camera-credential-wizard]] (#88, bug B). Same page as the Live path.

---

## Why (the gap today)

`sentinel-app/src/routes/live/+page.svelte` plays MSE-over-WebSocket (go2rtc `api/ws`, fed to a
`MediaSource`). Two problems:

- **The "LIVE" badge is cosmetic.** It renders `<StatusDot state="online"/> LIVE` whenever a camera
  is merely *selected* (`live/+page.svelte:268-271`) — not tied to the WS being open, the first frame
  arriving, or the `sourceBuffer` having data. It reads LIVE through every failure (the bug-A black
  screen showed green LIVE the whole time).
- **No connecting/error state.** Before the first frame the user sees a black 16:9 box
  (`.frame{background:#000}`); on failure it *stays* black with no explanation. There's a page-level
  "Loading streams…" for the device list only (`:221-222`), nothing per-stream.

The connection lifecycle the code already goes through (`startStream`, `:49-160`) gives us the exact
signals: WS `onopen`, the `{type:"mse"}` reply → `addSourceBuffer`, the first `appendBuffer`, WS
`onclose`/`onerror`, and `MediaSource`/`sourceBuffer` errors.

## The state machine

Drive the UI from a real per-stream state, derived from those events:

- **`connecting`** — WS opening, or open but no frame appended yet. Show **our loader** (the dot-field
  spinner / brand loader), not a black void.
- **`live`** — first frame appended AND the video is progressing (`video.currentTime` advancing /
  `sourceBuffer` has buffered range). Only now show the **LIVE** badge.
- **`error`** — WS failed to open, closed early, `addSourceBuffer`/codec unsupported, or no frame
  within a timeout (~10s). Show a clear message + a **Retry**, and — where we can tell — *why*
  (e.g. "This camera's video codec isn't supported in your browser" for the HEVC/MSE case, vs a
  generic "Couldn't connect to the live stream").
- **`stalled`** — was live, frames stopped (WS dropped / buffer starved). Show a subtle reconnecting
  indicator and auto-retry.

## Phase 1 — track playback truth

- In `startStream`, set a reactive `phase` from the lifecycle: `onopen`, the MSE reply, the first
  `appendBuffer` (→ live), `onclose`/`onerror` (→ error), and a first-frame watchdog timer (→ error
  on timeout). Reset on stream switch (the `$effect` at `:163-169`).
- "Live" should require an actual advancing playhead, not just an open socket — hook `video`'s
  `timeupdate`/`progress` or the `onUpdateEnd` seek logic (`:81-96`) to confirm frames are flowing.

## Phase 2 — honest UI

- **Badge:** render LIVE only in the `live` phase. In `connecting` show a muted "Connecting…"; in
  `error` a red "Offline"/"No signal". Reuse `StatusDot` states honestly.
- **Overlay:** our brand loader centered on the video well while `connecting`; a clear error card with
  Retry while `error`. No more bare black box.
- Named helpers return the badge/overlay spec from `phase` (house style, no markup ternaries).
- Keep the SD/HD toggle; a codec-unsupported error on HD should suggest trying SD.

## Phase 3 — tie into box health (operator side)

- The silent black screen also hid an *operator* problem (bug A: go2rtc not reachable over the
  tailnet). Surface a coarse signal: if the gateway can't reach the box's go2rtc, the error copy
  should hint "the appliance's live video isn't reachable" and link to the Boxes page, rather than
  implying the camera is at fault. (The box already reports `tailscale_state` serving/up/offline.)

## Verification

- Working camera → loader briefly, then video + LIVE badge. No green LIVE over black.
- Kill the stream (stop go2rtc / wrong stream name) → loader → after the watchdog, a clear error +
  Retry, badge NOT green.
- HEVC-only camera in a browser without `hvc1` MSE → the codec-specific message, with "try SD".
- Switch cameras rapidly → state resets cleanly, no stuck loader/leaked WS.

## Open decisions (grill before building)

1. **What exactly counts as "live"** — first `appendBuffer`, or an advancing `currentTime`? The
   latter is truer (frames actually rendering) but slightly later. Leaning advancing-playhead with a
   short grace so the badge isn't jumpy.
2. **How much failure detail to show** — a single generic error, or thread codec/host/auth causes?
   Some are knowable client-side (codec unsupported), some aren't (why the box bridge closed). Leaning:
   client-knowable causes get specific copy, the rest get a good generic + Retry.
3. **Auto-retry policy** — retry `error`/`stalled` automatically (how many times / backoff) vs
   manual Retry only? Leaning auto-retry a few times on `stalled`, manual on hard `error`.

## Out of scope (v1)

- WebRTC fallback for browsers/codecs where MSE fails (go2rtc's stream.html does this; the app is
  MSE-only in cloud mode). A real fix but a bigger one — note it, don't build it here.
- Multi-camera grid health at a glance (this is the single-stream view).
