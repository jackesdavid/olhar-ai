# Spec: 015 — Web migration: Live video

> **Status: DONE (code) — web migration unit (M1).** `cargo check` + `npm run build` clean;
> ratatui `src/` untouched. The live stream + go2rtc binary + cross-OS WebRTC are
> runtime-verified by the operator (camera/hardware/OSes). `validate_device` (persists
> creds + rtsp_url) also fixes the 014 snapshot gap.

## Goal

The operator watches a saved camera's live feed in the web app.

## User Value

Real-time eyes on a camera — the core surveillance payoff — in the new UI.

## Approach (per feasibility doc)

Browsers can't speak RTSP, so a bundled **go2rtc** sidecar transcodes RTSP→**WebRTC**
(H.264 passthrough, sub-second). The backend generates go2rtc's config from the saved
devices, starts the sidecar, and the Live screen embeds go2rtc's WebRTC player per device.

## Requirements

- **`validate_device(ip, username, password)` command**: ONVIF-test the creds, fetch the RTSP
  URI (GetStreamUri), and **persist `username`/`password`/`rtsp_url` onto the saved device**.
  (Also fixes the snapshot RTSP fallback / 014's missing-rtsp_url gap.)
- A go2rtc config generated from the device registry (one stream per device with a saved
  `rtsp_url`), keyed by a URL-safe stream name (e.g. `cam_<sanitized-ip>`).
- Start the go2rtc sidecar (best-effort spawn from `src-tauri/binaries/go2rtc`; binary fetched
  by the user per a DOWNLOAD.md — the agent may not download/run third-party binaries).
- Commands: `ensure_streams()` (write config + (re)start go2rtc, return `[{ip, name, player_url}]`).
- Live screen: a device picker → embed go2rtc's player (`http://localhost:1984/webrtc.html?src=<name>`).

## Acceptance Criteria

- [ ] `validate_device` persists creds + `rtsp_url` onto the device (verifiable: snapshot then works).
- [ ] `ensure_streams` writes a valid go2rtc.yaml from devices and (re)starts the sidecar.
- [ ] Live screen lets the operator pick a saved camera and shows its WebRTC feed.
- [ ] `cargo check` + `npm run build` clean; ratatui `src/` untouched.

## Dependencies

- 014 Devices (the registry + rtsp_url).

## Risks

- **Cross-OS webview media** (Linux WebKitGTK WebRTC off-by-default; Windows codecs) — the
  go/no-go, validated by the operator on real hardware/OSes. Not validatable in CI.
- go2rtc binary is the user's to fetch; productionizing as an externalBin sidecar is a follow-up.

## Notes

Snapshot stays a still (014). This is the live RTSP feed only.
