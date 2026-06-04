# Spec: Live RTSP Stream Playback

> **Status: SHIPPED — retrospective record.** Reverse-engineered from `src/stream/`.
> Documents existing behavior; no code change implied.

## Goal

The operator watches a camera's live RTSP feed rendered directly in the console.

---

## User Value

Real-time eyes on a location without a separate viewer app, so the operator stays in one
console during an incident.

---

## Requirements

- Spawn ffmpeg to pull a camera's RTSP stream and decode frames.
- Render frames to the terminal (grayscale, downscaled to Braille glyphs).
- Auto-reconnect on transient network errors with bounded backoff.
- Distinguish fatal errors (auth) from transient ones (timeout) and stop appropriately.
- Support one live stream at a time, usable from device detail and chat modals.

---

## Acceptance Criteria

- [x] Launching a feed shows live frames updating in the terminal.
- [x] A transient drop triggers automatic reconnection (bounded attempts).
- [x] A fatal/auth error stops with a failed state rather than looping forever.
- [x] Closing the stream reaps the ffmpeg child process.
- [x] RTSP transport (TCP/UDP/Auto) follows the operator's config setting.

---

## Dependencies

- 002-camera-discovery (rtsp_url / credentials)
- 012-config (RTSP transport preference)

---

## Risks

- External ffmpeg dependency must be installed on the host.
- A wedged ffmpeg child could leak — mitigated by health supervision + reap on exit.

---

## Notes

Out of scope: recording to disk, multi-stream grid. State machine:
Connecting / Live / Reconnecting / Failed.
