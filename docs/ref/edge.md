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
- **Record** (`start`/`stop`): `start()` snapshots the ring from the last IDR (clean
  keyframe → no black frames) **and** installs the live sink under the same `MemRing` lock,
  so the boundary has no gap/overlap. One `ffmpeg -f h264 -i pipe:0 -c:v copy out.mp4`:
  pre-roll bytes are written first, then a relay thread feeds live frames to its stdin.
  `stop()` clears the live sink → channel closes → relay drains → ffmpeg gets EOF and
  finalises the MP4. No concat step. Sidecar `<clip>.json` holds friendly name + metadata.

## Motion detection (OpenCV MOG2) — `src/motion.rs`

The edge owns **continuous** motion detection (it's the only thing near the cameras). A
manager thread reconciles **one watcher thread per camera** (every 30s, against the live
device list). Each watcher opens **go2rtc's local RTSP re-stream** (`rtsp://127.0.0.1:8554/cam_<ip>`
— the preview/substream, already authenticated to the camera by go2rtc, so the watcher needs no
creds and never touches the camera directly) via OpenCV `VideoCapture`, and per frame: downscale
→ **MOG2** adaptive background subtraction → `morphologyEx(OPEN)` denoise → `findContours`.
(go2rtc's RTSP is bound localhost-only in the generated config; override the base with
`MOTION_GO2RTC_RTSP`.) When the foreground share clears `MOTION_FLOOR_PCT` **and** a blob ≥
`MOTION_MIN_AREA` exists **and** `MOTION_MIN_INTERVAL_SECS` has elapsed since the last emit, it
`POST /config/motion`s a normalized event `{camera_ip, score, area, blobs, bbox}` (box-token).

The edge is a **pure sensor** — it emits ONE normalized signal per camera, graph-agnostic.
Per-graph **sensitivity** is a cloud-side threshold (`OnEvent("motion") → If score >= …`), so
many graphs at different sensitivities share the same single MOG2 pass. Tunables (env):
`MOTION_FPS` (6), `MOTION_FLOOR_PCT` (0.5), `MOTION_MIN_INTERVAL_SECS` (10), `MOTION_MIN_AREA`
(300), `MOTION_PROC_W`/`MOTION_PROC_H` (480×270), `MOTION_MOG2_HISTORY` (500), `MOTION_MOG2_VAR` (16).

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
