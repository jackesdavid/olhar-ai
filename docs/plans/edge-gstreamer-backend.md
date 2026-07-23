# Edge media backend — GStreamer alternative to ffmpeg, per-box flag + measured — Implementation Plan

**Board card:** #138 Edge GStreamer media backend (per-box ffmpeg↔gstreamer switch + measurement)

## Status (Phase 1 + Phase 2a shipped, in-progress)
- **Phase 1 done:** `RecordingConfig.media_backend` (`"ffmpeg"` default / `"gstreamer"`, `recording.rs`)
  — no migration needed, `recording_config` is already a per-box jsonb column. `capture::Backend`
  enum threaded through `capture::ensure`/`supervise` (called from `frames.rs` alongside
  `face_capture_stream`). `state::gstreamer_ok()` added (mirrors `ffmpeg_ok()`) + `X-Edge-Gstreamer`
  heartbeat header (not yet persisted/surfaced on control — that's cheap to add once a box actually
  needs the badge). A local `EDGE_MEDIA_BACKEND` env override was added for single-box testing
  without a control-side toggle UI (not yet built).
- **Phase 2a done — live MJPEG decode (A1) via GStreamer:** `capture::spawn_gst` runs
  `rtspsrc ! decodebin ! videoconvert ! videorate ! video/x-raw,framerate=6/1 ! jpegenc ! fdsink` —
  **codec-agnostic** (no per-H264/H265 branch) and emits the exact same wire format as ffmpeg
  (concatenated JPEGs on stdout), so `read_mjpeg` needed zero changes. **Software-only** decode, per
  the plan's open decision #5 (clean apples-to-apples vs ffmpeg's un-accelerated baseline).
  A stale/wrong `media_backend=gstreamer` on a box without `gst-launch-1.0` silently falls back to
  ffmpeg (`Backend::resolve`) — never leaves a camera with no working decoder.
- **Validated live on the Jetson Orin Nano test box** (`192.168.10.200`, aarch64/Tegra, real RTSP
  camera): the pipeline decoded 88/88 valid JPEG frames at the target 6fps with zero errors; a full
  `sentinel-edge` build with `EDGE_MEDIA_BACKEND=gstreamer` ran the supervised decoder loop for 80+s
  with no watchdog kills/respawns (`capture-decoder[…] backend=gstreamer`, one stable `gst-launch-1.0`
  process, ~17% CPU on a 6-core Tegra — software H264 decode).
- **Load-bearing discovery:** this Jetson's stock `gstreamer1.0-plugins-*` did NOT include a
  software H264 decoder — `decodebin` failed with "missing plugin" until `gstreamer1.0-libav` was
  installed. **`install.sh` (Phase 3) must add `-libav` alongside `-{base,good,bad,ugly}`** or the
  SW-only path silently has no decoder on a fresh box (`gstreamer_ok()` only checks the CLI binary,
  not codec coverage — worth hardening later).
- Open decisions resolved by the above: **#1 subprocess model** (`gst-launch-1.0` via `proc::command`,
  confirmed working) · **#2 flag transport** (extend `RecordingConfig`, confirmed) · **#4 v1 scope**
  (A1 first, confirmed) · **#5 SW-only for 2a** (confirmed, and it's what surfaced the `-libav` gap).
- **Control-side toggle + health shipped:** `gstreamer_ok` is now a real column (migration `0069`,
  mirrors `ffmpeg_ok`) reported on `X-Edge-Gstreamer`, surfaced on Settings → Edge boxes. Once a box
  reports it, a per-box `<select>` (ffmpeg/gstreamer) PATCHes `recording_config.media_backend`
  (spreading the box's existing config so custom pre_roll/storage/etc. survive the write) — no
  tenant-wide toggle, this is per-box on purpose for A/B, and it's gated behind `gstreamer_ok` so it
  can't be flipped on a box that would just silently fall back anyway. Takes effect on next restart
  (`RecordingManager` reads `media_backend` once at boot, like every other `recording_config` field).
- **`install.sh` updated:** `--media-backend gstreamer` / `SENTINEL_MEDIA_BACKEND=gstreamer` installs
  `gstreamer1.0-{tools,plugins-base,plugins-good,plugins-bad,plugins-ugly,libav}` (Linux, mutable +
  rpm-ostree-layered) or the Homebrew equivalents (macOS) — opt-in only, ffmpeg install is untouched
  and still the default. Writes `EDGE_MEDIA_BACKEND=$MEDIA_BACKEND` to `edge.env` either way.
- **Phase 2b (recording mux, A2) tried and REJECTED — real blocker, not a config miss:**
  `fdsrc fd=0 ! h264parse ! mp4mux faststart=true ! filesink` fails with **"Buffer has no PTS"**
  for a raw Annex-B elementary stream fed over stdin. Confirmed on the Jetson test box with a
  *clean* H264 sample (captured via `ffmpeg -c:v copy`, ruling out the go2rtc-repackaged-NAL
  corruption that motivated the edge's own custom `H264Reassembler` in the first place) — tried
  `filesrc` (rules out a pipe-specific issue) and `fdsrc do-timestamp=true`/`is-live` (not a
  property on this fdsrc), with and without an explicit `framerate` cap before/after `h264parse`.
  A synthetic `videotestsrc ! x264enc ! h264parse ! mp4mux` pipeline muxes fine in the same
  environment — so `mp4mux` itself works, the problem is specifically that `h264parse` can't
  invent PTS for a byte-stream with no timing metadata (this camera's SPS has no VUI timing info).
  The standard fix is `appsrc` with manually-assigned buffer PTS — which needs `gstreamer-rs`
  bindings, contradicting decision #1 (subprocess-only, because #79 rules out the edge linking
  anything). **`spawn_muxer` now ignores `media_backend` and always uses ffmpeg for A2/A3** —
  `RecordingConfig.media_backend` only drives `capture.rs`'s A1 live decode for now. Revisit only
  if the project ever revisits the "no bindings" constraint.
- **Phase 3 hwaccel (GStreamer HW-decode element) tried, shelved — inconclusive:** an explicit
  `rtph264depay ! h264parse ! nvv4l2decoder ! nvvidconv ! …` pipeline DOES work on the Jetson (85/85
  valid JPEGs) once `gstreamer1.0-plugins-bad` is also installed (`h264parse` lives there, not in
  `-good`) — but a rough `ps %cpu` comparison against the SW `decodebin` path showed no clear win
  (~15% HW vs ~17% SW, single noisy samples, not a rigorous run). Given the added fragility (an
  extra system package, a Jetson/Tegra-only element name, bypassing `decodebin`'s codec-agnostic
  autoplug so H264 vs H265 would need separate pipeline branches again) for an unproven gain, this
  is shelved — exactly per the plan's own gating ("once SW parity is measured", Phase 0, which
  hasn't been done rigorously via `EdgeMetrics` yet). Don't re-attempt without a proper Phase 0 A/B.
- **Phase 2c (snapshot fallback, A5) decided: keep ffmpeg.** It's a one-off credential-test
  snapshot (Devices → Test credentials), not a continuous operation — no measurement value, and
  `app_core::onvif::client::snapshot_from_rtsp` is shared library code with only one caller
  (edge `http.rs`), so converting it would add cross-crate backend plumbing for zero payoff.
- **Not yet done:** Phase 0 (tag `EdgeMetrics` heartbeat with active backend for a rigorous A/B —
  CPU was eyeballed via `ps`, not the metrics pipeline, both passes so far).
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
- **Bundling / first test box:** ffmpeg is a Linux system package (`install.sh:195-230`,
  apt/dnf/rpm-ostree) and bundled on macOS/Windows. **The first test + measurement box is the
  operator's macOS dev box** (see the `mac-edge-test-box` setup: manual `/tmp/sentinel-edge`
  install) — GStreamer there is just `brew install gstreamer gst-plugins-{base,good,bad,ugly}
  gst-libav`, so `gst-launch-1.0` is on PATH and we can measure immediately with **zero installer
  bundling**. macOS hwaccel is **videotoolbox → the `vtdec` GStreamer element**, so the HW-decode
  angle is testable on day one. Production bundling (Linux system `gstreamer1-plugins-*`; macOS pkg;
  Windows installer) comes later, once the numbers justify shipping it.

**Test setup (Phases 0–2 run here first):** the operator's **macOS** edge box (`mac-edge-test-box`):
`brew install gstreamer gst-plugins-{base,good,bad,ugly} gst-libav`, then flip that one box's
`media_backend` to `gstreamer` and compare `EdgeMetrics` against its ffmpeg baseline on the same
cameras. macOS decode HW = `videotoolbox`/`vtdec`. Linux appliance validation + install packaging
follow once the Mac numbers look right.

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
