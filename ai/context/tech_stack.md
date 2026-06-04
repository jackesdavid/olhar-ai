# Tech Stack

Bishop is a single-binary **Rust** application (edition 2021). There is no web frontend,
no server, and no database — the "frontend" is a terminal UI and the "backend" is the same
process talking to external HTTP APIs and subprocesses.

## UI / Rendering

- `ratatui` (0.29) — terminal UI, the primary render target
- `ratatui-wgpu` (0.4, optional `crt` feature) — self-rendered CRT window via `wgpu` +
  `winit`, with phosphor glow / scanline post-processing
- `image` (0.25) — JPEG/image decode for camera snapshots; downscaled to Braille/quadrant
  glyphs for the terminal

## External Services (HTTP via `ureq` 2)

- **Claude / Anthropic API** — AI chat + vision (model from `ANTHROPIC_MODEL`,
  default `claude-opus-4-8`)
- **ElevenLabs** — TTS (voice synthesis) and Scribe (speech-to-text)
- **Telegram Bot API** — sendMessage / sendPhoto / getUpdates (pairing + alerts)
- **ONVIF** — camera discovery and WS-Security SOAP control (`sha1`, `base64` for digest)

## Media / Audio

- `rodio` (0.19, optional `audio` feature) — local sound effects + TTS playback
- `cpal` (0.15, optional `audio` feature) — microphone capture for STT
- **ffmpeg** (external subprocess) — pulls RTSP streams, frames piped back to the app

## Persistence

- Flat JSON files at the repo root, via `serde_json` (1):
  `devices.json`, `map.json`, `mapview.json`, `alerts.json`, `notifications.json`,
  `config.json`
- `chrono` (0.4) — RFC3339 timestamps on alerts/deliveries
- `dotenvy` (0.15) — load `.env` for secrets

## Build Features / Modes

- Default features: `audio + crt`
- `--no-default-features` — silent, no audio
- `--tty` — terminal only, no GPU window

## AI Tools (development)

- Claude Code (this SDD workflow runs here)
