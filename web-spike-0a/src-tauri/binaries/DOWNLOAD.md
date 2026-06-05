# Drop the go2rtc binary here

The 0b spike spawns `binaries/go2rtc` (an RTSP→WebRTC gateway). Download the build for your
OS into this folder, named exactly `go2rtc` (no extension on macOS/Linux), and `chmod +x` it.
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

# Windows (x86_64) — name it go2rtc.exe and adjust the path in src/lib.rs (spawn_go2rtc)
#   https://github.com/AlexxIT/go2rtc/releases/latest/download/go2rtc_win64.zip
```

> Verify the binary's checksum/signature against the go2rtc release page before running it —
> it's third-party software fetched from the internet.

This file and the binary live under `src-tauri/`. Production would instead bundle go2rtc as a
Tauri **externalBin sidecar** (`tauri.conf.json → bundle.externalBin`, named with the target
triple) so it ships inside the app and is lifecycle-managed.
