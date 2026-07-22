# Edge media backend — GStreamer alternative to ffmpeg, per-box flag + measured — Implementation Plan

**Board card:** #138 Edge GStreamer media backend (per-box ffmpeg↔gstreamer switch + measurement)
**Goal:** Let each edge box run **GStreamer instead of ffmpeg** for every media operation the edge
does, switchable per box via a flag pushed from control, so we can A/B **measure CPU/mem** and decide
whether GStreamer is lighter. ffmpeg stays the default and both coexist behind the flag **for now**.

**North star:** the per-box flag is a **migration path, not a permanent fork**. If the numbers hold
up (GStreamer measurably lighter, at parity on every touchpoint below), the endgame is to **flip the
default to GStreamer and then retire ffmpeg entirely** — drop the ffmpeg subprocesses, the
`install.sh` ffmpeg dependency, and (pending the go2rtc `frame.jpeg` decision, B3 below) the ffmpeg
system/bundled binary. The coexistence phase exists only to prove the numbers before that removal.

## Grounding — every ffmpeg touchpoint on the edge (the seam surface)
All edge ffmpeg use goes through one spawn helper — `crate::proc::command(...)`
(`sentinel-edge/src/proc.rs:9`) — except the app-core snapshot fallback (raw `Command`). Seven
touchpoints (verified inventory):

| # | Operation | Location | Mechanism | HW-accel today |
|---|---|---|---|---|
| A1 | **Live MJPEG decode** (hot per-camera frame path) | `capture.rs:68-93` (`spawn_ffmpeg`) | `proc::command("ffmpeg")` → MJPEG on `pipe:1` | **none** |
| A2 | **Recording mux** (H264 ring → mp4, `-c:v copy`) | `recording.rs:335-347` | `proc::command("ffmpeg")` stdin pipe | none (stream-copy) |
| A3 | Clip duration probe | `recording.rs:849-858` (`probe_duration`) | `proc::command("ffprobe")` | n/a |
| A4 | HW-decode capability probe | `hwaccel.rs:59-116` | `proc::command("ffmpeg")` ×2 | detects, never applies |
| A5 | Single-frame snapshot fallback | `crates/app-core/src/onvif/client.rs:253` | raw `Command::new("ffmpeg")` | none |
| A6 | ffmpeg presence check | `state.rs:414` (`ffmpeg_ok`) | `ffmpeg -version`, `OnceLock` | n/a |
| B3 | go2rtc-internal ffmpeg for `/api/frame.jpeg` | go2rtc runtime; consumers `app-core go2rtc.rs:112-142`, `frames.rs:185-249` | go2rtc spawns ffmpeg itself | none |

**Facts that shape the design:**
- **A1 is the heavy one** — a long-lived ffmpeg per camera doing CPU MJPEG encode with **no
  hwaccel**. This is what "ffmpeg feels heavy" almost certainly is. It is the prime measurement
  target and the first GStreamer slice.
- **hwaccel is detected but never applied.** `hwaccel.rs` returns `vaapi|qsv|cuda|videotoolbox|
  v4l2m2m|none` but that method is **only reported to the cloud** (health gating,
  `state.rs:459-466` → `X-Edge-Hwaccel`) — no ffmpeg command passes `-hwaccel`. So a GStreamer path
  using HW-decode elements (`vaapih264dec`/`nvh264dec`/`vtdec`) could be a **real win**, not just
  parity.
- **Recording capture is already pure-Rust** (retina RTSP + own H264 depacketizer, `recording.rs`
  §F); ffmpeg is ONLY the mp4 muxer (A2) + duration probe (A3).
- **go2rtc streaming is native** (RTSP/WebRTC/MSE, no ffmpeg). Only `/api/frame.jpeg` uses go2rtc's
  internal ffmpeg (B3) — and A1's warm decoder already bypasses it for the hot path; B3 is only the
  cold-start snapshot fallback. So **go2rtc stays out of scope**.
- **The edge links nothing** (#79, musl static release build) → the GStreamer path must be
  **spawned `gst-launch-1.0` subprocesses** (mirroring the ffmpeg-subprocess model via
  `proc::command`), **not** `gstreamer-rs` bindings (which would link libgstreamer and break the
  static build). This is the load-bearing design choice.
- **Config-pull seam exists:** the edge pulls `GET /config/recording` → `RecordingConfig`
  (`recording.rs:874`, struct `recording.rs:103`) and `GET /config/cameras` (`state.rs:622`), both
  bearer-auth by `BOX_TOKEN`. Per-box column precedent: `tenant_boxes.hwaccel` (migration `0064`).
- **Measurement already exists:** `EdgeMetrics` (#62, `sentinel-edge/src/metrics.rs`,
  `app_core::edge_metrics`) samples CPU/mem/disk/GPU and ships it on the heartbeat
  (`X-Edge-Metrics`). `frames.rs:219-249` already splits go2rtc-grab vs edge-decode cost. Reuse for
  the A/B.
- **Bundling:** ffmpeg is a Linux system package (`install.sh:195-230`, apt/dnf/rpm-ostree) and
  bundled on macOS/Windows. GStreamer needs the mirror — v1 targets the **Linux appliance** (system
  `gstreamer1-plugins-*`); macOS/Windows bundling is deferred (heavier plugin graph).

## Phase 0 — Measure first (baseline, no backend change)
Quantify "ffmpeg is heavy" and set up the A/B before writing any GStreamer.
- Tag the `EdgeMetrics` heartbeat with the **active `media_backend`** (default `ffmpeg`) so control
  can group CPU/mem by backend. Normalise by camera count (already known from `/config/cameras`).
- Control-side: a simple per-box CPU/mem-over-time comparison (reuse the box health/metrics already
  stored). Leverage the existing `frames.rs` grab-vs-decode split.

## Phase 1 — The flag + the `MediaBackend` seam (no behaviour change)
- **Migration** (next number — `ls crates/app-core/migrations/`): `ALTER TABLE tenant_boxes ADD
  COLUMN IF NOT EXISTS media_backend text` (mirror `0064`). NULL → edge treats as `ffmpeg`.
- **Control:** serve it to the edge by adding `media_backend` to the `RecordingConfig` response
  (`/config/recording`, `recording.rs:103`) — least new plumbing (already fetched, drives the
  recording+capture managers). Operator sets it via a per-box settings API/toggle (box registry).
- **Edge:** a `MediaBackend` enum (`Ffmpeg` | `Gstreamer`) read from the pulled config, threaded
  into `capture::ensure` (A1), `RecordingManager` (A2/A3), snapshot (A5), and the probes (A4/A6).
  Refactor today's ffmpeg calls behind `MediaBackend::Ffmpeg` — **byte-identical behaviour**. Add
  `gstreamer_ok()` (parallel to `ffmpeg_ok`, `state.rs:414`) reported like `X-Edge-Ffmpeg`.
- Result: ffmpeg still the only working impl and the default; the switch is inert until Phase 2.

## Phase 2 — GStreamer backend, operation by operation (measure each slice)
Each is a buildable slice: implement the `Gstreamer` branch as a `gst-launch-1.0` pipeline, flip a
test box to `gstreamer`, compare `EdgeMetrics` vs an ffmpeg box on the same cameras.
- **2a — Live MJPEG decode (A1), FIRST (heaviest, the whole point):**
  `rtspsrc location=rtsp://127.0.0.1:8554/cam_<ip> ! rtph264depay ! h264parse ! <decoder> !
  videorate ! video/x-raw,framerate=6/1 ! jpegenc quality=… ! fdsink` where `<decoder>` is
  `avdec_h264` (SW) or the HW element for the box's detected hwaccel. Replaces `spawn_ffmpeg`; the
  supervisor/watchdog (`capture.rs:95-167`) stays.
- **2b — Recording mux (A2/A3):** `fdsrc ! h264parse ! mp4mux faststart=true ! filesink` (or
  `splitmuxsink`) fed the same ring bytes; duration via `gst-discoverer-1.0` (replaces ffprobe).
- **2c — Snapshot fallback (A5):** a one-frame `rtspsrc ! … ! jpegenc ! fdsink` (behind the same
  flag in app-core, or keep ffmpeg for the rare fallback — open decision).

## Phase 3 — hwaccel + install/bundling
- Map the box's detected hwaccel (`hwaccel.rs`) → GStreamer HW-decode element for 2a
  (`vaapih264dec` / `nvh264dec` / `vtdec`) — the potential win ffmpeg doesn't currently take.
- `install.sh`: install `gstreamer1-plugins-{base,good,bad,ugly}` + `-libav` (Linux) when the box
  runs gstreamer; `gstreamer_ok()` gates the switch. macOS/Windows bundling deferred.

## Open decisions (grill before building)
1. **`gst-launch-1.0` subprocess vs `gstreamer-rs` bindings** → recommend **subprocess** (preserves
   #79 "edge links nothing" + musl static release; reuses `proc::command`). Bindings only if a
   pipeline needs tight Rust control (appsrc for the recording ring).
2. **Flag transport:** extend `RecordingConfig`/`/config/recording` (recommended, least plumbing) vs
   a dedicated per-box `/config/box` endpoint.
3. **go2rtc `frame.jpeg` (B3):** leave on ffmpeg (recommended — go2rtc is a Go binary; 2a's warm
   decoder already covers the hot path) vs eliminate the fallback entirely.
4. **v1 operation scope:** A1 first (heaviest), then A2, then A5; A4/A6 become `gstreamer_ok` + a GST
   hwaccel probe. Confirm this order.
5. **Apply hwaccel opportunistically in 2a** (a win ffmpeg doesn't get today) or keep 2a SW-only for
   a clean apples-to-apples first measurement?

## Phase 4 (future, gated on the numbers) — retire ffmpeg
Only once GStreamer has proven measurably lighter and at parity across A1–A6 on real boxes:
flip the default `media_backend` to `gstreamer`, roll it out, then **remove ffmpeg** — the edge
subprocesses (A1/A2/A3/A5), the `ffmpeg_ok`/hwaccel-probe ffmpeg calls (A4/A6), the `install.sh`
ffmpeg dependency, and the macOS/Windows bundled ffmpeg binary. The go2rtc `frame.jpeg` residual
(B3) must be resolved first (eliminate the fallback, or accept go2rtc keeping its own ffmpeg).

## Out of scope (v1 — the coexistence phase, not the north star)
- **Removing ffmpeg** — that's Phase 4, gated on the measurements. v1 keeps both, ffmpeg default.
- go2rtc internals / native streaming (unchanged); its `frame.jpeg` ffmpeg residual (B3).
- macOS/Windows GStreamer bundling (Linux appliance first).
