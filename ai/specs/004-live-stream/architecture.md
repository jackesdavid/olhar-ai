# Architecture Notes — Live RTSP Stream Playback

## Components

- `src/stream/domain.rs` — `LiveStream` state machine (Connecting/Live/Reconnecting/Failed),
  spawn params, reconnect attempts/backoff, activity guard.
- `src/stream/client.rs` — ffmpeg subprocess spawn, frame channel, reader thread.
- `src/stream/mod.rs` — re-exports.
- `src/ui/image.rs` — frame → Braille glyph downscaling.
- Consumers: `src/device/` (detail screen) and `src/agent/` (`show_live_feed` tool modal).

## APIs

- ffmpeg CLI (external subprocess) reading RTSP; frames piped back over a channel.

## Data Flow

Spawn params (rtsp_url, transport) → `client` starts ffmpeg → reader thread decodes frames
(960×540 default, ~12 fps) → frames published over an mpsc channel → view downsamples to
terminal cells each tick. The state machine drives auto-reconnect (max ~6 attempts,
STALL_RECONNECT ≈ 120 ticks) and separates transient vs fatal failures.

## Important Decisions

- ffmpeg subprocess over an in-process RTSP decoder — simpler, robust, reuses a known tool.
- Single concurrent stream — bounds resource use and matches the single-operator model.

## Risks

- External ffmpeg dependency; child-process lifecycle must be supervised and reaped.
