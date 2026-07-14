# Windows edge appliance

**Board card:** #80 Windows edge — installer, service, self-update (ToDo)
**Goal:** Ship `sentinel-edge` on Windows, so a customer can turn a PC they already own into the
appliance — no Linux box, no reflash. Same binary contract as everywhere else: ONVIF discovery,
go2rtc, motion, tracking, recording, tailnet.

**Depends on [[edge-pure-rust-motion]] (#79)** — see "The gate" below. Related:
[[edge-frame-hub]] (#77).

---

## Why this is worth doing

Every prospective customer already has a Windows PC. Today the appliance story is "buy a mini PC and
let us flash it" — for a small shop that's a real barrier, and for a pilot it's an absurd one. A
Windows edge turns "can I try it?" into a download.

It also widens what we can sell: the same box can be the shop's front-desk PC.

## The gate: OpenCV

**Do not attempt this before #79.** The edge's one native dependency is OpenCV, and on Windows that
means a vcpkg/manual OpenCV build, shipping `opencv_world4xx.dll` next to the exe, and keeping the
two in lockstep forever — on top of an auto-updater that would then have to replace DLLs too. #79
deletes OpenCV outright (it survives in exactly two files and ten calls after #77, nine of which map
onto crates we already depend on). With it gone, the Windows build is a plain
`cargo build --release --target x86_64-pc-windows-msvc`.

Everything below assumes that. If #79 is rejected, this card gets 3× bigger and should be re-planned.

## Phase 1 — the binary

- Target `x86_64-pc-windows-msvc`. Cross-building from macOS is possible (`cargo-xwin`) but a
  Windows CI runner / VM is the honest path — GitHub Actions is billing-blocked, so likely a local
  Windows VM to start.
- Audit the POSIX assumptions in `sentinel-edge/src`:
  - **Paths**: `/etc/sentinel-edge/edge.env`, `/opt/sentinel-edge/recordings` (`recording.rs:135`),
    the `EDGE_CACHE` default. Move to `%ProgramData%\Sentinel\` via a small `paths` module rather
    than sprinkling `#[cfg(windows)]`.
  - **`state::platform()`** reads `/etc/os-release` → add a Windows branch (`Windows 11 (x86_64)`),
    since it feeds the Boxes page badge.
  - **Process spawning**: go2rtc (`go2rtc.rs`) and the `ffmpeg`/`ffprobe` children
    (`recording.rs:301`, `:787`) — `.exe` suffixes, and no console window
    (`CREATE_NO_WINDOW`) or every clip pops a black box on the user's screen.
  - `ffmpeg_ok()`'s PATH probe.

## Phase 2 — the dependencies

- **go2rtc**: ships an official `go2rtc_win64.zip`. Same config, same localhost binds.
- **ffmpeg**: bundle a static `ffmpeg.exe` + `ffprobe.exe` (gyan.dev / BtbN builds) next to the edge
  rather than asking the user to install anything.
- **Tailscale**: the Windows installer + `tailscale.exe up --authkey …`. It runs as a service
  already; `tailscale serve --bg 1984` works the same. Note `install.sh`'s existing lesson — treat a
  non-zero `up` as non-fatal and always attempt `serve`.

## Phase 3 — install + service

`install.sh` is bash and registers systemd/launchd. Windows needs its own:

- **`install.ps1`**, served by control the same way (`sentinel-control/src/http.rs:27` routes
  `/install.sh`, embedded with `include_str!`) → add `/install.ps1`, embedding
  `sentinel-edge/install.ps1`. One-liner install: `irm https://…/install.ps1 | iex`.
- It must: download the binary + go2rtc + ffmpeg, write the env file, install Tailscale, register
  the service, start it.
- **Service**: a real Windows service (the `windows-service` crate) or `sc.exe create` +
  **failure actions** (`sc failure … reset= 0 actions= restart/5000`). The restart command
  (`docs/ref/edge.md`) works by the edge calling `exit(0)` and trusting the supervisor to relaunch
  it — that contract must hold on Windows, or the Boxes-page Restart button silently kills boxes.
  A Scheduled Task with "restart on failure" is the fallback if the service route drags.

## Phase 4 — self-update

`maybe_self_update` (`sentinel-edge/src/main.rs:37`) shells out to `install.sh --update`, which
overwrites the binary in place. **Windows cannot replace a running `.exe`.** The standard dance:

1. rename the running `sentinel-edge.exe` → `sentinel-edge.exe.old` (allowed while running),
2. write the new exe to the real path,
3. `exit(0)` → the service supervisor starts the new one,
4. delete the `.old` on next boot.

Do this in an `install.ps1 --update` so the logic lives with the installer, mirroring today's shape.

## Phase 5 — release plumbing

- Asset name `sentinel-edge-windows-x86_64.exe` in the dist server (`/ibbf/edge-dist/`); control's
  `/edge-latest` + the Boxes update badge already key off `LATEST_EDGE_VERSION`, so nothing there
  changes — but `task edge-publish`'s `uname` case statement needs a Windows arm (or a separate
  `edge-windows-publish`).
- Decide how a Windows box is built/uploaded given GitHub Actions is billing-blocked (a local
  Windows VM + `scp`, most likely).

## Verification

- Fresh Windows 11 machine, no dev tools: `irm …/install.ps1 | iex` → the box appears in the Boxes
  page with the right platform badge, tailnet `serving`, ffmpeg ok.
- Discovery finds an ONVIF camera on the LAN; Live plays; a snapshot returns.
- Motion + tracking events reach the cloud (this is the #79 detector on a new OS — re-run its
  golden-frames check here).
- Record a clip → it plays; with #78's switch on, the local file is dropped after upload.
- Reboot the PC → the service comes back on its own. Hit Restart from the Boxes page → it comes back.
- Trigger an auto-update from an older version → it actually swaps the running exe.

## Open decisions (grill before building)

1. **Service or Scheduled Task?** A real service is the right answer for an appliance (starts before
   login, survives logout). It's also more code. Leaning service, with `sc.exe` failure actions.
2. **Bundle ffmpeg/go2rtc, or fetch them at install time?** Bundling makes the installer big but
   hermetic and offline-installable; fetching keeps it small but adds failure modes on a customer's
   network. Leaning bundle.
3. **Signed installer?** Unsigned, Windows SmartScreen will scare every customer we're trying to
   make comfortable. A code-signing cert is ~US$100–400/yr. Probably required for a real product —
   decide before, not after, the first customer sees the warning.
4. **Windows Defender / firewall**: the edge binds localhost and go2rtc pulls RTSP from the LAN;
   the installer likely needs a firewall rule. Confirm on a clean machine.
5. **Do we support Windows at all as a *sold* appliance, or only as a "try it on your own PC" tier?**
   That answer changes how much we invest in signing + support.

## Out of scope (v1)

- ARM Windows.
- A GUI/tray app. The edge is a headless service; the web app is the interface.
- Windows Server / domain-joined specifics.
