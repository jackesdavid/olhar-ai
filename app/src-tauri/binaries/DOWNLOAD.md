# Drop the go2rtc binary here

Unit 015 (Live video) spawns `binaries/go2rtc` — an RTSP→WebRTC gateway. The backend
generates `<app_data_dir>/go2rtc.yaml` from your saved devices and starts go2rtc pointed
at it; the `/live` page then plays each camera over WebRTC.

Download the build for your OS into **this folder**, named exactly `go2rtc` (no extension on
macOS/Linux) or `go2rtc.exe` (Windows), and `chmod +x` it on Unix.
Releases: https://github.com/AlexxIT/go2rtc/releases/latest

```bash
# macOS (Apple Silicon)
curl -L -o go2rtc https://github.com/AlexxIT/go2rtc/releases/latest/download/go2rtc_mac_arm64
chmod +x go2rtc

# macOS (Intel)
curl -L -o go2rtc https://github.com/AlexxIT/go2rtc/releases/latest/download/go2rtc_mac_amd64
chmod +x go2rtc

# Linux (x86_64)
curl -L -o go2rtc https://github.com/AlexxIT/go2rtc/releases/latest/download/go2rtc_linux_amd64
chmod +x go2rtc

# Linux (arm64)
curl -L -o go2rtc https://github.com/AlexxIT/go2rtc/releases/latest/download/go2rtc_linux_arm64
chmod +x go2rtc

# Windows (x86_64) — name it go2rtc.exe (the backend looks for binaries/go2rtc.exe on Windows)
#   https://github.com/AlexxIT/go2rtc/releases/latest/download/go2rtc_win64.zip
```

> Verify the binary's checksum/signature against the go2rtc release page before running it —
> it's third-party software fetched from the internet.

## Notes

- The binary is **git-ignored** and is never committed (see `app/.gitignore`). Each developer
  / build machine drops it in locally.
- The backend spawns it **best-effort**: if the binary is absent, the app logs a warning and
  keeps running — only live video is disabled. There is no need to download anything to build.
- The config is **generated**, not hand-edited: validate each camera's credentials on the
  **Devices** page (this persists its `rtsp_url`), then the `/live` page's "Rescan streams"
  regenerates `go2rtc.yaml` and restarts the sidecar. go2rtc does H.264 passthrough (no
  re-encode) for sub-second latency and serves its API + WebRTC player on
  `http://localhost:1984`.
- Production would instead bundle go2rtc as a Tauri **externalBin sidecar**
  (`tauri.conf.json → bundle.externalBin`, named with the target triple) so it ships inside the
  app and is lifecycle-managed.
