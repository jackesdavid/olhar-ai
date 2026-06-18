# Edge appliance (`sentinel-edge`)

The only thing that touches cameras. Config-light: no database — it pulls the camera list
from control and caches it to `cameras-cache.json` (offline fallback). Standalone crate (not
in a workspace) so it ships independently; links `app-core` by path.

## Boot + loops (`src/main.rs`)

1. `logbuf::init()` — capturing logger (tees to stderr + a bounded ring; see Logs below).
2. `tailscale::ensure_up()` — join the tailnet + `serve` go2rtc (see Tailscale below).
3. `AppState::boot()` — pull cameras from control (cache fallback), resolve RTSP, write cache.
4. ffmpeg check (`state::ffmpeg_ok()`), go2rtc respawn, initial `push_streaming()`.
5. Threads: **discovery loop** (every 5 min: `rediscover()` + re-assert `tailscale serve`);
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

## Health reporting + Restart

On the heartbeat the edge sends `X-Edge-Tailscale` (`serving|up|offline|disabled`, from
`tailscale_state()`) and `X-Edge-Ffmpeg` → stamped on `tenant_boxes` → the **Boxes page**
shows per-box badges + a warning banner when offline / not-serving / ffmpeg-missing.

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

`install.sh` (served at `control/edge/install.sh`) installs Tailscale + ffmpeg + the edge
binary + go2rtc, writes `/etc/sentinel-edge/edge.env`, and registers a service (systemd on
Linux, launchd on macOS — bootout-before-bootstrap so a fresh install over an old one doesn't
fail with `Load failed: 5`). Key env: `CONTROL_URL`, `BOX_TOKEN` (binds the box to its
tenant), `EDGE_PUBLIC_HOST`, `EDGE_OWN_GO2RTC=1`, `GO2RTC_BIN/CONFIG`, `EDGE_CACHE`,
`TS_AUTHKEY/TS_HOSTNAME/TS_TAGS`, and a baked `PATH`. `--update` swaps just the binary.

> The macOS edge is a **test box**; real appliances are Linux (where `install.sh` is fully
> headless end-to-end). See [mac-edge-test-box memory] for the local box specifics.
