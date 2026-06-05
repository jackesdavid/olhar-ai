# Sentinel

A desktop **monitoring & automation assistant** for the home and the workplace.
Discover ONVIF cameras, watch live video, raise and deliver alerts, lay out a 3D
floor plan, talk to an AI assistant that can act on your devices, and automate it
all with graph-based workflows — voice in and out.

Built with **Tauri + SvelteKit (Svelte 5) + Rust**. UI: shadcn-svelte + Threlte
(Three.js). AI: Claude (Anthropic). Voice: ElevenLabs (TTS + Scribe STT). Live
video via a bundled go2rtc sidecar (RTSP → WebRTC/MSE).

## Repository layout

| Path        | What it is                                                              |
|-------------|-------------------------------------------------------------------------|
| `app/`      | **The product.** Tauri shell + `app-core` Rust crate + SvelteKit UI.    |
| `legacy/`   | The original **ratatui** terminal prototype (`bishop`), frozen.         |
| `ai/`       | Spec-driven-development specs/roles (`001…013` retro, `014…022` web).    |
| `docs/`     | Web-stack feasibility study + the feature-by-feature migration plan.    |
| `Taskfile.yml` | [go-task](https://taskfile.dev) runner for the web app.              |

## Run

```sh
brew install go-task node rust ffmpeg   # prerequisites
cp .env.example .env                     # then fill in API keys (see below)
task setup                               # npm install + cargo fetch + go2rtc
task dev                                 # launch the desktop app (hot reload)
```

`task` lists everything; `task build` builds the frontend, `task bundle` produces
a distributable app.

## Configuration

All secrets live in a single gitignored `.env` at the repo root (loaded by `task`
and the VS Code debugger):

- `ANTHROPIC_API_KEY` — the AI assistant (chat + vision). Required for chat.
- `ELEVENLABS_API_KEY` (+ optional `ELEVENLABS_VOICE_ID` / `…_MODEL_ID` /
  `…_STT_MODEL`) — voice replies and dictation.
- `TELEGRAM_BOT_TOKEN` / `TELEGRAM_PAIRING_CODE` — alert delivery to recipients
  (also settable in-app on the Recipients screen).

Each capability degrades gracefully when its key is absent.

## Legacy

The `legacy/` directory holds the original ratatui terminal app this project grew
from. It is frozen — the web app is the maintained product. See
`legacy/README.md` to build/run it.
