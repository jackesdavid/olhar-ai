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
| A1 | **Live decode** (RTSP → raw frames, hot per-camera path) | ffmpeg MJPEG on `pipe:1`, **no hwaccel** (`capture.rs`) | `rtspsrc ! <hwdec> ! appsink` → motion/detector | edge motion + tract detector | 1 | ✅² | ✅¹¹ | 🟡 |
| HW | **Hardware decode** (currently detected, never applied) | `hwaccel.rs` reports, never uses | `vtdec`(Mac) / `nvv4l2decoder`(Jetson) / `amcviddec`(Android) / `vaapi`·`nvh264dec`(Linux) / `d3d11h264dec`(Win) | A1 pipeline | 1 | ✅¹ | ✅¹¹ | 🟡 |
| A2 | **Recording mux** (H264 ring → fragmented mp4) | ffmpeg `-c:v copy` (`recording.rs`) | `appsrc` (real per-frame PTS) `! h264parse ! mp4mux` | clip store / playback | 3 | ✅⁵ | ⬜ | 🟡 |
| A3 | **Clip duration probe** | `ffprobe` (`recording.rs`) | **no probe at all** — duration = captured frame span | recording metadata | 3 | ✅⁶ | ⬜ | 🟡 |
| A4 | **HW-decode capability probe** | ffmpeg `-hwaccel` list (`hwaccel.rs`) | `media::hw_decoder()` — registry element presence | health report | 5 | ✅¹⁰ | ⬜ | 🟡 |
| A5 | **Snapshot fallback** (single-frame, cold) | raw `ffmpeg` (`onvif/client.rs`) | one-shot `rtspsrc ! decodebin ! jpegenc` (feature-gated) | **credential probe only** — no longer a snapshot fallback | 2 | ✅⁴ | ⬜ | 🟡 |
| A6 | **ffmpeg presence check** | `ffmpeg -version` (`state.rs`) | skipped when GStreamer owns media | health gating | 5 | ✅⁹ | ⬜ | 🟡 |
| B3 | **Snapshot from warm feed** (`/api/frame.jpeg`, "Get current frame" node) | go2rtc → internal ffmpeg | serve the A1 warm JPEG (`capture::latest_fresh`); cold path only as fallback | workflow node, app-core `go2rtc.rs`, gateway | 2 | ✅³ | ⬜ | 🟡 |
| G1 | **RTSP ingest from cameras** | go2rtc per-camera streams | `rtspsrc` (shared A1 ingest) | all downstream | 1 | ✅² | ✅¹¹ | 🟡 |
| G2 | **Live MSE over WebSocket** (`/api/ws`) | go2rtc | `mp4mux fragment-duration` → WS, **contract-identical** | **gateway `proxy.rs` → web player + mobile WebView player** | 4 | ✅⁷ | 🟡¹¹ | 🟡 |
| G4 | **Stream list / player assets** (`/api/streams`, `/stream.html`) | go2rtc-served, gateway passthrough | edge-served equivalents or drop if unused | gateway passthrough | 4 | ⬜ | ⬜ | ⬜ |
| G5 | **go2rtc sidecar lifecycle** (spawn/config/health) | edge manages the go2rtc process | **not started at all** on a GStreamer box | edge `state.rs`, `main.rs` | 5 | ✅⁹ | ⬜ | 🟡 |
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
git). **INTEGRATED into the edge (2026-07-24):** the linked path now lives in `sentinel-edge` behind
the `gst-linked` cargo feature (`media.rs` `Backend::GstreamerLinked`, `capture.rs` `run_gst_linked`) —
the ffmpeg-A1 successor, producing JPEG into `cam.latest` via the SAME public `ensure`/`latest_fresh`
contract (downstream unchanged). A gated integration test publishes a valid JPEG from the live camera
through the real public API. Default build unbroken (static, no gstreamer). Still 🟡 overall: Jetson
gate + default-flip open.

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
| 3 | A2 recording mux + A3 duration | ✅ | ⬜ | 🟡 |
| 4 | G2 MSE contract + G4/G6 serving/tailnet | ⬜ | ⬜ | ⬜ |
| 5 | retire go2rtc (G5) + ffmpeg (A4/A6) | ⬜ | ⬜ | ⬜ |

**Done = every row ✅ on both boxes, and neither ffmpeg nor go2rtc installed.** That is the green light
for the phone-as-edge (Android) card.

³ **B3 partial (2026-07-24):** the **edge's own `/snapshot/:ip`** now serves the warm decoded JPEG
first (`capture::latest_fresh`, <2s), falling back to the cold go2rtc→ONVIF/ffmpeg path only when no
fresh frame exists — killing go2rtc's internal ffmpeg (and its ~490ms cold start) for every consumer
of that endpoint (e.g. control's line-drawing UI). **Still open:** the workflow **"Get current frame"**
(`app-core ports.rs` `EdgeFrameSource`) runs in *sentinel-flow* and still calls go2rtc
`/api/frame.jpeg` directly over the tailnet; pointing it at the edge's snapshot endpoint (the edge
binds 0.0.0.0:8788, so it is reachable) — **DONE (2026-07-24)**: `EdgeFrameSource` now tries the
edge's own `/snapshot/:ip` first (derived from the go2rtc base: tailnet `https://host` → `:8080`,
local `:1984` → `:8788`, per `tailscale serve --https=8080 8788`) and falls back to go2rtc. A box
whose edge API is unreachable (older edge) is remembered process-wide so it pays the 2s probe once,
not per grab. 4 unit tests cover the URL derivation. Both B3 consumers — the edge endpoint and the
cloud workflow node — now prefer the warm frame; go2rtc's internal ffmpeg is only a fallback.

⁴ **A5 + the great snapshot collapse (2026-07-24).** The operator's call: *"we should have only one —
the warm buffer."* `/snapshot/:ip` now has **exactly one** source — the supervised decoder — starting
it on demand (on the same stream the hub would pick) and waiting out the cold start. The old
four-way chain (warm → go2rtc `frame.jpeg` → ONVIF `GetSnapshotUri` → one-shot RTSP), **two of whose
rungs spawned ffmpeg**, is gone; if the decoder can't produce a frame the camera is unusable for
live/recording/detection anyway, so failing honestly beats a silent slow path. `snapshot_from_rtsp`
survives for the genuinely different job — the **credential probe** (`/probe/:ip`), validating
arbitrary creds against a camera that may not be configured — and now runs on GStreamer where the
runtime exists: app-core gained an optional `gst-linked` feature that the **edge forwards**, so the
cloud services (control/flow), which have no GStreamer, keep the ffmpeg path untouched.

⁵ **A2 built, awaiting real record+playback verification (2026-07-24).** The blocker the plan had
recorded as REJECTED — `h264parse` cannot invent PTS for a raw byte stream — is gone now that the
pivot made linked bindings the design: `src/gst_mux.rs` feeds `appsrc ! h264parse ! mp4mux` one
buffer per access unit with **PTS taken from the frame's real capture time** (`H264Frame.at`), never
an assumed framerate (which would yield clips that play at the wrong speed). Supporting change: the
pre-roll and the live channel now carry `(bytes, Instant)` per frame instead of a flattened blob —
shared plumbing, so the ffmpeg path was re-verified (its bytes land back-to-back exactly as the old
concatenation did). Both build configs clean, tests pass in both. **NOT yet proven:** no clip has
been recorded and played back — that is the only test that catches a PTS error, because a wrong PTS
produces a playable-looking file with the wrong duration/speed rather than a loud failure.

⁶ **A3 done — by deleting the question, not porting it (2026-07-24).** A2 made every frame carry its
capture time, so a clip's duration is simply `last.at - first.at`: no `ffprobe` subprocess, and no
GStreamer `discoverer` either. It is also *truer* than both old answers — wall-clock counted the wait
for the first IDR and any camera dropout as footage, and re-reading the file only recovers what the
muxer already knew. `probe_duration` survives as a last resort for a clip whose span was never
recorded.

⁷ **G2 built, installed and measured at parity with go2rtc on the Mac (2026-07-24).**
`sentinel-edge/src/gst_mse.rs` serves the `/api/ws` contract itself (`rtspsrc → h264parse → mp4mux →
appsink`, fanned out over a broadcast channel; one pipeline per stream, started on the first viewer,
dropped after the last).

**Measured against the running go2rtc over the same wall-clock window** (25 s, same camera, both
sockets open at once — measuring them in sequence is worthless, scene motion swings the byte counts
by 40 %):

| | edge (GStreamer) | go2rtc |
|---|---|---|
| WS messages per fragment | **1.0** | 1.0 |
| median gap between fragments | **66 ms** | 66 ms |
| bytes per fragment | **1233** | 1257 |
| first playable fragment | **0.33 s** | 0.93 s |
| `codecs=` advertised | `avc1.640016` (true) | `avc1.640029` (over-declared) |

Two things had to be fixed to get there, both worth knowing:

- **`cmafmux` → `mp4mux`.** `cmafmux` closes a fragment only on a keyframe, so its cadence was the
  camera's GOP whatever `fragment-duration` said: one fragment every **2.0 s**, first frame at 5.6 s.
  `mp4mux` fragments on a timer and matches go2rtc. It is also the only portable choice — `cmafmux`
  ships in gst-plugins-rs and is **absent from the JetPack 6 GStreamer** (see the Jetson row).
- **One WS message per fragment.** `mp4mux` emits a box per buffer, so fragments went out as three
  messages (`moof`, `mdat` header, payload) — 845 in 20 s against go2rtc's 293, each one an
  `onmessage` + `appendBuffer` in the player. The `mdat` header declares its total size, so the
  fragment is emitted the moment its last byte lands: no added latency, unlike waiting for the next
  `moof`.

**Still open before this is ✅:** repoint the **gateway** upstream from go2rtc `:1984` to the edge
`:8788`. Until that flip the browser still plays go2rtc's stream, so nothing user-visible has changed.

**How the feasibility was proven first.** The one
unknown that decided Stage 4 was *"can GStreamer emit fMP4 a browser `MediaSource` accepts, live from
the camera?"*. Answer: **yes**, via `cmafmux` (gst-plugins-rs `isobmff`, ships with the Homebrew
GStreamer). Proof in `experiments/gst-spike/src/bin/stage4.rs` — 32 live segments off the real camera;
dumped to a file the byte stream walks **`ftyp+moov` then `styp+moof+mdat`×N, structurally intact end
to end** (159,886/159,886 bytes) and `ffprobe`s as H264 High/level 2.2 640×360, 9.33 s of a 10 s
capture. The `codecs=` string to advertise in the `/api/ws` handshake comes straight from the `avcC`
(`01 64 00 16` → **`avc1.640016`**).

**The gotcha that cost the whole debug session — `cmafmux` requires DTS, `rtph264depay` gives none.**
Symptom: the pipeline goes **EOS within ~0.1 s having produced zero bytes**, and the *only* bus message
is `rtspsrc … streaming stopped, reason error (-5)` — which points at the source, not the mux. The real
message is visible **only** under `GST_DEBUG=fmp4mux:6`: `Require DTS for video streams`. Two further
traps while chasing it: (a) `gst-launch … ! fakesink` "succeeding" in 0.1 s is *not* success, it is that
same instant EOS — always assert on **bytes produced**, not on exit status; (b) `cmafmux` only accepts
`stream-format=avc` (never `byte-stream`), so a capsfilter after `h264parse` is mandatory. Fix used in
the spike: **`h264timestamper`** between `h264parse` and the capsfilter. That element lives in
gst-plugins-bad, so for the edge itself prefer setting `DTS = PTS` in a pad probe from Rust — the
cameras are B-frame-free, and it keeps Android/Jetson free of one more plugin dependency.

⁸ **Jetson surveyed over SSH (2026-07-24) — nothing run there yet.** The box is a **Jetson Orin Nano
Developer Kit** (not the old Nano): Ubuntu 22.04.5, Jetson Linux R36.5.0 (JetPack 6), **GStreamer
1.20.3**, aarch64. Present: `rtspsrc`, `rtph264depay`, `h264parse`, **`nvv4l2decoder`**, **`nvvidconv`**,
`videoconvert`, `jpegenc`, `appsrc`, `appsink`, **`mp4mux`** — so A1/HW, A2 and G2 all have their
elements. Absent: **`cmafmux`** and `h264timestamper`, which is what forced G2 onto `mp4mux`. The
`gstreamer-1.0` dev headers are installed (gstreamer-rs will build); there is no `cargo` yet. Expect
`nvv4l2decoder` to output NVMM memory, so the A1 pipeline needs `nvvidconv` where the Mac uses
`videoconvert`.

⁹ **THE MAC NOW RUNS WITH NO SIDECARS AT ALL (2026-07-24).** `ps` shows **zero** media processes —
no go2rtc, no ffmpeg — and `/health` reports `owns_go2rtc: false`. Verified on that box, in that state:
warm snapshot **HTTP 200 in 0.0008 s**, live `/api/ws` serving fMP4 (`avc1.640016`), and a recording
of **12 s / 258 KB from a fresh ring session**.

What made it possible: the edge dials each camera **itself, once**, and `tee`s the H264 to the decode
branch (JPEG for motion + detectors) and the mux branch (fMP4 for live). The recording ring dials the
camera too, which was go2rtc's last consumer. Ownership is decided in `AppState::boot`, so the
config-change respawns follow the same rule instead of each having to remember it.

**Four traps, each of which failed silently or misleadingly:**
1. The stored `rtsp_url` carries **no credentials** — they live on the device and go2rtc's config
   injected them. Dialling the bare URL gets `Unauthorized` and the supervisor retries forever: the
   snapshot went 502 and Live produced nothing. Now injected via `rtsp_url_with_creds`, same as the
   sidecar, and covered by tests — the logic that broke had been unreachable from one.
2. **`retina` refuses inline credentials** (`URL must not contain credentials`); they must be session
   options. It had never surfaced because go2rtc's re-serve needed no auth at all. Split at the
   **last** `@` — a password containing `@` is legal, and splitting at the first truncates the host.
3. The stored URLs carry go2rtc's **`#media=video`** fragment, which is its syntax and not RTSP:
   `rtspsrc` takes it as part of the path and the camera 404s, which reads as bad credentials.
4. Both `tee` branches are **`leaky=downstream`** — a stalled consumer must never back the RTSP
   source up and take the other branch down with it.

**Still open on the Mac:** the gateway's `/api/frame.jpeg` passthrough and the tailnet's `:443` both
still point at go2rtc `:1984`, which no longer exists on this box (the edge API is on `:8080`). That
is G4/G6 and it means the *gateway* snapshot path is broken here even though the edge's own is not.

¹⁰ **A4 done as a side effect of making the pipeline portable (2026-07-24).** `media::decode_branch()`
picks the decode half of the ingest from **what the box's GStreamer registry actually has**, and
`media::hw_decoder()` reports the same finding for health. Probing the registry is both the
capability answer and the selection, so there is one source of truth and a box missing its
accelerator degrades to software rather than failing to start — where `ffmpeg -hwaccels` could only
ever answer, never select. The Jetson is why it exists: `nvv4l2decoder` outputs into **NVMM** memory
that `videoconvert` cannot read (it needs `nvvidconv`), and pairing them wrong produces no error at
all — the pipeline simply never yields a frame.

¹¹ **Jetson: the ingest runs, decode is NVDEC (2026-07-24).** Rust installed, the edge **builds on
aarch64 with `gst-linked`** (10m12s), 30 unit tests pass, and the both-branches integration test
passes against a real camera: ONE connection, `tee`'d to `nvv4l2decoder ! nvvidconv` and to the live
muxer. Not yet run as the box's actual edge — that is the remaining step, and it needs a window on a
client machine.

**Two GStreamer-version traps, and they point the same way — the newer runtime is the more forgiving
one, so what the Mac accepts is not evidence of what ships:**
- **DTS.** `mp4mux` on 1.20 refuses an unset DTS (`Could not multiplex stream`, `dts:
  99:99:99.999999999` in `qtmux`'s log). The Mac's 1.28 tolerates it, so the PTS→DTS pad probe had
  been dropped as unnecessary when we moved off `cmafmux`. Restored.
- **`avc` vs `byte-stream`.** A `tee` forces ONE output format on the shared `h264parse`, and the
  branches disagree: the decoder takes byte-stream, `mp4mux` on 1.20 accepts only `avc` and answers a
  byte-stream query with **EMPTY** caps. The pipeline then dies at `rtspsrc` with `not-negotiated
  (-4)` — naming neither the element that refused nor why. The mux branch now re-parses to `avc`.

## ✅ Verified live on the Mac edge (2026-07-24)

Stages 0-3 are no longer just "built" — they were exercised on the running box with a real camera:

| Check | Result |
|---|---|
| Linked backend actually active | **zero media subprocesses** — no `ffmpeg`, no `gst-launch`; only go2rtc remains, as expected at this stage |
| B3 warm snapshot | HTTP 200, valid 640×360 JPEG, **0.019 s** (the go2rtc `frame.jpeg` path it replaces measured ~490 ms) |
| **A2 mux + A3 duration** | clip 383,846 B; edge reported **23 s** from the frame span; `ffprobe` on the file read **23.196 s** — the two agree, which is exactly the check that catches a PTS error (a wrong PTS makes them diverge) |
| Muxer in isolation | 117 real camera access units → EOS → 141 KB mp4 (`experiments/gst-spike` bin `stage3`) |

**A scare worth recording:** the first clips appeared to be created at 0 B and then vanish. Neither is
a fault — `mp4mux faststart` writes the file at EOS (so 0 B *during* recording is correct), and the
box has `delete_local_after_upload` on (card #78), so both the `.mp4` and its `.json` are removed once
the clip is durable in the cloud. The clip lives in the cloud recordings list, not on the box.
