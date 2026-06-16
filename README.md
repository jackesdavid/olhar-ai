# Sentinel

A multi-tenant cloud **monitoring & automation assistant** for the home and the
workplace. Discover ONVIF cameras (via your edge appliance), watch live video, raise
and deliver alerts, lay out a 3D floor plan, and automate it all with graph-based
workflows.

Built with **SvelteKit (Svelte 5) + Rust**. UI: shadcn-svelte + Threlte (Three.js).
AI: Claude (Anthropic). Live video via go2rtc on the edge (RTSP → WebRTC/MSE),
bridged to the browser by the streaming gateway.

## Repository layout

| Path             | What it is                                                          |
|------------------|--------------------------------------------------------------------|
| `sentinel-app/`  | **The web app.** SvelteKit UI (served static via nginx).           |
| `crates/app-core/` | Shared UI-agnostic Rust lib (engine, nodes, repos, ports).       |
| `sentinel-control/` `-gateway/` `-flow/` `-edge/` | The cloud services + on-prem edge. |
| `legacy/`        | The original **ratatui** terminal prototype (`bishop`), frozen.    |
| `ai/`            | Spec-driven-development specs/roles + the vision sidecar.          |
| `docs/`          | Feasibility study + the feature-by-feature migration plan.         |
| `Taskfile.yml`   | [go-task](https://taskfile.dev) runner.                            |

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
