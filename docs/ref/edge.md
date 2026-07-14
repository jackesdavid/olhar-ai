# Edge appliance (`sentinel-edge`)

The only thing that touches cameras. Config-light: no database — it pulls the camera list
from control and caches it to `cameras-cache.json` (offline fallback). Standalone crate (not
in a workspace) so it ships independently; links `app-core` by path.

## Boot + loops (`src/main.rs`)

1. `logbuf::init()` — capturing logger (tees to stderr + a bounded ring; see Logs below).
2. `tailscale::ensure_up()` — join the tailnet + `serve` go2rtc (see Tailscale below).
3. `AppState::boot()` — pull cameras from control (cache fallback), resolve RTSP, write cache.
4. ffmpeg check (`state::ffmpeg_ok()`), go2rtc respawn, initial `push_streaming()`.
5. `motion::spawn()` — the continuous motion watcher manager (see Motion below).
6. Threads: **discovery loop** (every 5 min: `rediscover()` + re-assert `tailscale serve`);
   **config long-poll** (`reload(25)`, then drain `check_commands()` → rescan/restart);
   **log push** (every 3s, ships new ring lines to `/config/logs`).

## Tailscale (the Live dependency)

go2rtc is localhost-only; the gateway reaches it via `tailscale serve --bg 1984` over the
tailnet. `tailscale::ensure_up()` runs `tailscale up --authkey …` then `serve_go2rtc()` —
and **always attempts serve even if `up` fails** (a macOS GUI-app daemon makes `up` exit
non-zero though the node is up). `serve_go2rtc()` is idempotent + re-asserted every 5 min.

**Enrollment key (per box):** control mints the `--authkey` when a box is created
(`sentinel-control/src/tailscale.rs`, OAuth client → Tailscale API). The key is
**reusable + non-ephemeral**, ~90 days (`TS_KEY_EXPIRY_SECS`) — so the edge re-auths on
every reboot from the key baked into `edge.env`, and the node keeps a stable MagicDNS name.
(It used to be single-use + ephemeral + 30 min, which dropped boxes off the tailnet on the
first reboot.) Re-keying an existing box = recreate it in Settings → Boxes and reinstall.

`install.sh` installs Tailscale headlessly so the appliance joins from the CLI:
- **Linux**: `curl -fsSL https://tailscale.com/install.sh | sh` (installs `tailscaled`).
- **macOS**: `brew install tailscale` (the open-source `tailscaled`, NOT the GUI app) as the
  invoking user (`HOMEBREW_NO_AUTO_UPDATE=1 HOMEBREW_NO_REQUIRE_TAP_TRUST=1`), then
  `tailscaled install-system-daemon` (launchd) + `tailscale up --authkey`. Uses a working
  `tailscale version` (not just PATH) as the install test so a stale GUI-app shim is replaced.

**`tailscale up` exit codes seen:** `1` = up failed (e.g., GUI app not connected); `127` =
the `tailscale` CLI isn't installed/working. Either way the box is off-tailnet → Live fails
while localhost go2rtc still serves. The box's state is reported as health (below).

## ffmpeg

go2rtc shells out to `ffmpeg` to produce JPEG frames — so **snapshots + the `GetCurrentFrame`
node fail without it** (`exec: "ffmpeg": executable file not found`) even though MSE/WebRTC
(Live video) plays. `install.sh` provisions it (apt/dnf/yum/apk; macOS brew) and bakes a
`PATH` into the service env so the launchd/systemd daemon (and the go2rtc child) find it.

## Discovery + IP auto-reconcile

ONVIF WS-Discovery (`app_core::onvif::discovery::discover()`) every 5 min. `rediscover()`
matches the sweep to cached devices by **stable ONVIF id**; for a camera whose IP changed it
repoints ip/xaddr, re-resolves RTSP, regenerates the `cam_<ip>` go2rtc streams, and pushes
`POST /config/device-moved` so the cloud device row + Live follow the move. A UI **Rescan**
(Devices page) sets a tenant command the edge runs immediately on its next poll.

go2rtc streams are named `cam_<ip-with-underscores>` (preview/substream) + `cam_<ip>_hd`
(main). `crates/app-core/src/go2rtc.rs` is the single source of truth for naming + config.

## Recording + pre-roll ring (`src/recording.rs`)

On-demand clip recording with an **in-memory H264 pre-roll ring** — no per-camera
ffmpeg ring process, no segment files on disk between recordings.

- **Ring** (`start_ring`): a background thread per camera opens an RTSP connection to
  go2rtc's local re-server (`rtsp://127.0.0.1:8554/cam_<ip>`). We use the `retina` crate
  **only for RTSP transport + RTP ordering** (raw `PacketItem::Rtp`, *not* `.demuxed()`)
  and do our own lenient H264 depacketization (`H264Reassembler`: single-NAL / STAP-A /
  FU-A → Annex-B access units, caches SPS/PPS and prepends them to keyframes). retina's
  built-in codec layer rejects go2rtc's repackaged NALs with `forbidden sequence 00 00 00`,
  which is why we bypass it. Access units live in a `VecDeque<H264Frame>` capped at
  ~`pre_roll_secs` of frames.
- **Cloud + eviction** (`src/clip_upload.rs`): on `stop`, the edge pushes the clip manifest to
  control; if the tenant's plan sells cloud recording (an active/trialing subscription whose plan
  has `retention_days > 0` — control's `upload_ticket`), control answers with a **presigned PUT**
  and the bytes go straight from the box to R2. With `recording_config.delete_local_after_upload`
  on (Settings → "Delete local copy once saved to the cloud"), the local `.mp4` **and its `.json`
  sidecar** are deleted once — and only once — control confirms the clip is durable, so the disk is
  a transient spool, not an archive. Off by default: with no cloud plan nothing is ever uploaded and
  the local file is the only copy. Both files go, because `StorageStats` reads `size_bytes` out of
  the sidecar and an orphan would make the box over-report its disk forever. Playback + Download
  both prefer the cloud for an `uploaded` clip, so an evicted local file is never missed.
- **Record** (`start`/`stop`): `start()` snapshots the ring from the last IDR (clean
  keyframe → no black frames) **and** installs the live sink under the same `MemRing` lock,
  so the boundary has no gap/overlap. One `ffmpeg -f h264 -i pipe:0 -c:v copy out.mp4`:
  pre-roll bytes are written first, then a relay thread feeds live frames to its stdin.
  `stop()` clears the live sink → channel closes → relay drains → ffmpeg gets EOF and
  finalises the MP4. No concat step. Sidecar `<clip>.json` holds friendly name + metadata.

## The frame hub — `src/frames.rs` (one fetch, one decode, per camera)

Motion and every object-tracking job want the same thing: the latest frame of a camera. They
used to each poll go2rtc's `/api/frame.jpeg` and decode the JPEG themselves, so a tracked camera
decoded one frame **up to three times** per cycle (motion's `imdecode`, the detector's internal
decode, and `detector::dimensions`'s second decode when the job had lines/zones).

Now a manager thread reconciles **one source thread per camera** (every 30s, against the live
device list). Each source pulls the latest frame from the local go2rtc `/api/frame.jpeg` (always
fresh — no RTSP client buffer to fall behind live motion, which is why `VideoCapture` was dropped),
**decodes it once**, runs that camera's MOG2 pass, and publishes a `Frame` to whoever subscribed.
A camera with four tracking jobs costs one decode, not five.

- The published `Frame` carries **plain byte buffers** (`bgr: Arc<Vec<u8>>` + `w`/`h`), never an
  `opencv::Mat` — `Mat` isn't `Sync`, so it can't be shared across the worker threads. A consumer
  that wants OpenCV wraps `bgr` back into a borrowed `Mat` for free.
- Publishing is a **latest-value slot** (`Mutex` + `Condvar`), not a queue: a slow consumer misses
  frames rather than backing the source up — the right trade for live video.
- `Frame.motion_at` carries **when this camera last had a qualifying MOG2 frame**, on *every*
  frame, so a consumer sampling slower than the hub (the tracker, at `detect_ms`) can't miss a
  burst of motion between the frames it looked at.

### Motion (MOG2) — `src/motion.rs` + `src/mog2.rs`

`motion.rs` is the **algorithm**, not the threading: one `MotionDetector` per camera holds that
camera's background model and is fed one decoded frame at a time by the hub. Per frame: downscale →
**MOG2** adaptive background subtraction → morphological open (denoise) → contours. When the
foreground share clears `MOTION_FLOOR_PCT` **and** a blob ≥ `MOTION_MIN_AREA` exists **and**
`MOTION_MIN_INTERVAL_SECS` has elapsed since the last emit, the hub `POST /config/motion`s a
normalized event `{camera_ip, score, area, blobs, bbox}` (box-token).

**It is pure Rust** (card #79). `src/mog2.rs` is a port of OpenCV's `BackgroundSubtractorMOG2`
(Zivkovic's adaptive GMM); morphology + contours come from `imageproc`, decode + resize from
`image`. The edge has **no native dependency at all** — see "No OpenCV" below.

**The throttle is on the emit, not on the verdict** — every frame gets a verdict; only the POST to
control is rate-limited. That's what lets the tracker's motion gate react immediately instead of
inheriting the 4s emit throttle.

The edge is a **pure sensor** — it emits ONE normalized signal per camera, graph-agnostic.
Per-graph **sensitivity** is a cloud-side threshold (`OnEvent("motion") → If score >= …`), so
many graphs at different sensitivities share the same single MOG2 pass. Tunables (env):
`MOTION_FPS` (4), `MOTION_FLOOR_PCT` (0.5), `MOTION_MIN_INTERVAL_SECS` (4), `MOTION_MIN_AREA`
(300), `MOTION_PROC_W`/`MOTION_PROC_H` (480×270), `MOTION_MOG2_HISTORY` (333), `MOTION_MOG2_VAR` (16).

> `MOTION_MOG2_HISTORY` is counted in **frames, not seconds** — it has to be rescaled with
> `MOTION_FPS` or the background model's time constant silently changes. 333 @ 4 fps ≈ the same
> ~83s of adaptation as the old 500 @ 6 fps.

### No OpenCV (card #79)

The edge used to link OpenCV for MOG2 — its **only** native dependency, and the source of most of
the pain in shipping it: the binary was soname-coupled to its build host (hence *two* Linux assets,
`-gnu` for Debian/OpenCV 4.6 and the default for Fedora/OpenCV 4.11), `install.sh` had to layer
OpenCV via `rpm-ostree` on Bazzite (**requiring a reboot**), and the appliance ran as a
self-contained container just to guarantee a matching soname. It also kept `sentinel-edge` out of
the cargo workspace and made every build need `libclang`.

`src/mog2.rs` replaces it. The port is **exact**: `cargo test --features opencv-parity` runs our
model and OpenCV's over the same frames and asserts the foreground masks agree on **every pixel of
every frame**. That is not a decorative test — it caught the one real bug in the port, and a looser
"within 1%" bound would have shipped it:

> OpenCV's automatic learning rate (what you get from `learningRate = -1`, which is how the edge has
> always called it) is **`α = 1/min(2·nframes, history)`**, not the constant `1/history`. It ramps:
> α starts at 1/2, so a freshly-started watcher settles on the scene in seconds instead of the ~83s
> a constant rate would take. With the constant, a box that just booted would have reported the
> whole scene as motion for its first minute and a half.

The `opencv` crate survives **only** as an optional dev dependency behind the `opencv-parity`
feature. It is not in the default build and never ships.

## Object tracking + the motion gate — `src/tracking.rs`

One worker per (tracking job × camera), each running YOLOX-Nano (`app_core::agent::detector`, tract,
CPU) on the hub's already-decoded BGR buffer via `detector::detect_bgr` — no second JPEG decode, and
no `dimensions()` call, since the hub already knows the frame size. `detect(&jpeg)` is untouched and
is still what sentinel-flow's cloud vision nodes use.

YOLOX is the most expensive thing on the box, and ungated it cost the same on an empty room at 3am
as on a busy shop floor. `should_detect()` now runs the detector only when **any** of:

1. **motion is active** — a qualifying frame within the last 3s (the raw verdict, not the emit);
2. **a track is still alive** — including `Lost` ones. Load-bearing twice over: a person standing
   perfectly still is absorbed into MOG2's background but is still there; and a track goes `Lost`
   the instant it stops matching, needing `max_age` more **unmatched detection cycles** to become an
   `object_exited`. Gating on `MultiTracker::count()` (confirmed only) would starve the tracker of
   exactly those cycles and the exit event would never fire;
3. **warmup** — a fresh worker's first 5s, to see whatever was *already static* in frame (a car
   parked before the box booted never generates motion);
4. **the idle sweep** — one cycle every 30s regardless (≈0.3% of a core), as insurance.

An idle scene therefore runs no inference at all. `TRACK_MOTION_GATE=0` restores the old always-on
behaviour.

**OpenCV is a native dep** (the only one) → the edge is NOT in the cargo workspace (keeps the
cloud image build pure-Rust). Build it from its own dir: `cd sentinel-edge && cargo build
--release`, needing libopencv + libclang. macOS: `brew install opencv` and pass
`LIBCLANG_PATH`/`DYLD_FALLBACK_LIBRARY_PATH=/Library/Developer/CommandLineTools/usr/lib` (also
baked into `task edge`). The binary links OpenCV **dynamically**, so the run host's OpenCV
major must match the build host's — build on the distro you run.

**Linux (Fedora/Bazzite):** cross-build from macOS in a Fedora container —
`task edge-linux-build` (uses `deploy/edge-linux/Dockerfile`, Fedora 42 = OpenCV 4.11;
`opencv 0.93` builds against it fine). `task edge-linux-publish` ships it to the VPS dist
server. Because the binary is OpenCV-soname-coupled and the target appliance is **Bazzite**
(immutable Fedora, no host `dnf install`), the edge there runs as a **self-contained container**
(`--target runtime` bakes matching OpenCV + ffmpeg + go2rtc + tailscale; run recipe in
`deploy/edge-linux/entrypoint.sh`). See [deploy ref](deploy.md) for the full build/publish path.

## Health reporting + Restart

On the heartbeat (the ~25s config poll) the edge sends `X-Edge-Tailscale`
(`serving|up|offline|disabled`, from `tailscale_state()`), `X-Edge-Ffmpeg`, and
`X-Edge-Platform` (`OS (arch)`, ASCII-only, e.g. `macOS (aarch64)` / `Bazzite (x86_64)`, from
`state::platform()` — distro name via `/etc/os-release`) → stamped on `tenant_boxes`
(`platform` col added in migration 0034) → the **Boxes page** shows per-box badges (platform,
tailnet state, ffmpeg) + a warning banner when offline / not-serving / ffmpeg-missing.

The edge also prints its version + platform in the first boot log line, and supports
`sentinel-edge --version`. `install.sh` echoes the version it's installing (from control's
`/edge-latest`) and, on an immutable host, runs `--version` once deps are active.

**Restart command**: Boxes page button → `POST /boxes/:id/restart` → control queues it
(`pending_restart`, per box) → the edge consumes it via `/config/commands` and `exit(0)`s, so
the launchd `KeepAlive` / systemd `Restart=always` supervisor relaunches it (re-running
`ensure_up` = reconnect + re-serve).

## Logs (in-UI)

`src/logbuf.rs` is a process-global bounded ring fed by the custom logger AND the go2rtc
child's stdout/stderr (target `go2rtc`). The push loop ships new lines to control
`/config/logs` → `box_logs` table + a `log_bus` broadcast → the web **/logs** page (full live
tail with device/level/text filters; deep-linkable `?device=<ip>` / `?box=<id>`).

## Install / env

`install.sh` (served at `control/edge/install.sh`, embedded in the control binary via
`include_str!`) installs Tailscale + ffmpeg + **OpenCV** (motion watcher runtime) + the edge
binary + go2rtc, writes `/etc/sentinel-edge/edge.env`, and registers a service (systemd on
Linux, launchd on macOS — bootout-before-bootstrap so a fresh install over an old one doesn't
fail with `Load failed: 5`). Key env: `CONTROL_URL`, `BOX_TOKEN` (binds the box to its
tenant), `EDGE_PUBLIC_HOST`, `EDGE_OWN_GO2RTC=1`, `GO2RTC_BIN/CONFIG`, `EDGE_CACHE`,
`TS_AUTHKEY/TS_HOSTNAME/TS_TAGS`, and a baked `PATH`. `--update` swaps just the binary.

> The macOS edge is a **test box**; real appliances are Linux (where `install.sh` is fully
> headless end-to-end). See [mac-edge-test-box memory] for the local box specifics.
