# GStreamer Migration — Functionality Matrix (progress tracker)

**Card:** #138 · **Plan:** [edge-gstreamer-backend.md](edge-gstreamer-backend.md) · **Goal:** retire
**go2rtc + ffmpeg** for one linked `gstreamer-rs` media path (Mac / Jetson / Android). **Driver:**
phone-as-edge portability — parity is enough, no need to be faster.

**Legend:** ⬜ not started · 🟡 in progress · ✅ done & tested · ⛔ blocked · N/A not applicable
**Deploy targets — ALL of these are clients, all must work** (operator, 2026-07-24): **Mac**
(VideoToolbox `vtdec`), **Linux** (VAAPI/NVDEC), **Jetson Nano** (aarch64 + NVDEC — also the Android
proxy), **Android** (MediaCodec `amcviddec`), **Windows — future** (D3D11 `d3d11h264dec` / Media
Foundation `mfh264dec`). The Mac + Jetson are the two active gates now; a capability counts as ✅ once
it passes on **both**, with the other platforms' HW-decode elements noted per row.

## Capability matrix — what each backend does for us

| # | Capability | Today (ffmpeg / go2rtc) | GStreamer replacement | Consumers | Stage | Mac | Jetson | Status |
|---|---|---|---|---|---|---|---|---|
| 0 | **Linked `gstreamer-rs` runtime** (not `gst-launch` subprocess) | n/a (new) | `gstreamer` + `gstreamer-app` crates, pipeline from Rust, `appsink` frame pull | the edge itself | 0 | ✅ | ⬜ | 🟡 |
| A1 | **Live decode** (RTSP → raw frames, hot per-camera path) | ffmpeg MJPEG on `pipe:1`, **no hwaccel** (`capture.rs`) | `rtspsrc ! <hwdec> ! appsink` → motion/detector | edge motion + tract detector | 1 | ✅² | ⬜ | 🟡 |
| HW | **Hardware decode** (currently detected, never applied) | `hwaccel.rs` reports, never uses | `vtdec`(Mac) / `nvv4l2decoder`(Jetson) / `amcviddec`(Android) / `vaapi`·`nvh264dec`(Linux) / `d3d11h264dec`(Win) | A1 pipeline | 1 | ✅¹ | ⬜ | 🟡 |
| A2 | **Recording mux** (H264 ring → fragmented mp4) | ffmpeg `-c:v copy` (`recording.rs`) | `appsrc ! h264parse ! mp4mux fragment-duration` | clip store / playback | 3 | ⬜ | ⬜ | ⬜ |
| A3 | **Clip duration probe** | `ffprobe` (`recording.rs`) | GStreamer `discoverer` / pipeline query | recording metadata | 3 | ⬜ | ⬜ | ⬜ |
| A4 | **HW-decode capability probe** | ffmpeg `-hwaccel` list (`hwaccel.rs`) | `gst_registry` element presence (`vtdec`/`nvv4l2decoder`…) | health report | 5 | ⬜ | ⬜ | ⬜ |
| A5 | **Snapshot fallback** (single-frame, cold) | raw `ffmpeg` (`onvif/client.rs`) | one-shot `rtspsrc ! <dec> ! jpegenc num-buffers=1` | credential test / cold snapshot | 2 | ⬜ | ⬜ | ⬜ |
| A6 | **ffmpeg presence check** | `ffmpeg -version` (`state.rs`) | removed (no ffmpeg) | health gating | 5 | ⬜ | ⬜ | ⬜ |
| B3 | **Snapshot from warm feed** (`/api/frame.jpeg`, "Get current frame" node) | go2rtc → internal ffmpeg | `jpegenc` off the A1 warm pipeline (`tee`) | workflow node, app-core `go2rtc.rs`, gateway | 2 | ⬜ | ⬜ | ⬜ |
| G1 | **RTSP ingest from cameras** | go2rtc per-camera streams | `rtspsrc` (shared A1 ingest) | all downstream | 1 | ✅² | ⬜ | 🟡 |
| G2 | **Live MSE over WebSocket** (`/api/ws`) | go2rtc | `mp4mux fragment-duration` → WS, **contract-identical** | **gateway `proxy.rs` → web player + mobile WebView player** | 4 | ⬜ | ⬜ | ⬜ |
| G3 | **WebRTC** (`/api/webrtc`) | go2rtc `webrtcbin` (minor use today) | `webrtcbin` | (minor) | 5 | ⬜ | ⬜ | ⬜ |
| G4 | **Stream list / player assets** (`/api/streams`, `/stream.html`) | go2rtc-served, gateway passthrough | edge-served equivalents or drop if unused | gateway passthrough | 4 | ⬜ | ⬜ | ⬜ |
| G5 | **go2rtc sidecar lifecycle** (spawn/config/health) | edge manages the go2rtc process | removed (no sidecar) | edge `state.rs`, `main.rs` | 5 | ⬜ | ⬜ | ⬜ |
| G6 | **Tailnet exposure** (MagicDNS TLS to go2rtc) | edge `tailscale.rs` → go2rtc | serve the GStreamer HTTP over the same tailnet | gateway reaches edge | 4 | ⬜ | ⬜ | ⬜ |

¹ **HW / Mac ✅ (RESOLVED 2026-07-24):** `vtdec` (VideoToolbox HW decode) delivers all 120/120 frames
via linked gstreamer-rs. The earlier "no valid frames" was a **synthetic-stream artifact** — `x264enc`'s
default profile negotiation produces a stream VideoToolbox HW rejects; forcing an explicit
`video/x-h264,profile=main` capsfilter fixes it. Real camera H264 always declares a standard profile in
its SPS, so `rtspsrc`-fed decode (Stage 1) should not hit this at all. Proven in `experiments/gst-spike/`.

² **A1/G1 Mac ✅ (Stage 1 media path, 2026-07-24):** the real A1 path — `rtspsrc` from the **live camera
on this Mac's edge** → `vtdec` HW decode → `appsink` — pulls frames into Rust on **both** substreams:
preview 640×360 and **HD 1280×720**, `vtdec` decoding each **directly, no profile-forcing** (real camera
SPS carries a standard profile). `experiments/gst-spike/` bin `stage1` (URL via `CAM_RTSP`, no creds in
git). Still 🟡: not yet wired into the edge's real MOG2/tract path, and the Jetson gate is open.

## Already pure-Rust (no migration needed — reused, feeds GStreamer)

| Capability | Where | Note |
|---|---|---|
| RTSP capture core | `recording.rs` (retina + H264 depacketizer) | stays; feeds `appsrc` |
| Motion detection | MOG2 pure-Rust port (`#79`) | stays; consumes A1 frames |
| Object detection | `tract` YOLOX | stays; consumes A1 frames |

## Stage roll-up

| Stage | Scope | Mac | Jetson | Status |
|---|---|---|---|---|
| 0 | Linked `gstreamer-rs` foundation + appsink | ✅ | ⬜ | 🟡 (Mac done, Jetson pending) |
| 1 | A1 live decode + HW decode + G1 ingest | 🟡 | ⬜ | ⬜ |
| 2 | B3 + A5 snapshots (`jpegenc`) | ⬜ | ⬜ | ⬜ |
| 3 | A2 recording mux + A3 duration | ⬜ | ⬜ | ⬜ |
| 4 | G2 MSE contract + G4/G6 serving/tailnet | ⬜ | ⬜ | ⬜ |
| 5 | G3 WebRTC + retire go2rtc (G5) + ffmpeg (A4/A6) | ⬜ | ⬜ | ⬜ |

**Done = every row ✅ on both boxes, and neither ffmpeg nor go2rtc installed.** That is the green light
for the phone-as-edge (Android) card.
