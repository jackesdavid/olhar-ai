# Phase-0a spike — Tauri 2 + SvelteKit + Rust-core reuse

Validates the recommended stack from `../docs/web-stack-feasibility.md`: a **Tauri 2**
desktop app with a **SvelteKit (static/CSR, Svelte 5)** frontend calling a **Rust
`#[tauri::command]`** that reuses **Bishop's alerts model**, end-to-end.

## What it proves
- `npm create tauri-app` `svelte-ts` template = **SvelteKit + `adapter-static` + `ssr=false`**
  (`svelte.config.js`, `src/routes/+layout.ts`) — exactly the target frontend, no manual setup.
- `src-tauri/src/lib.rs` exposes **`list_alerts`** — a spike copy of `bishop::alerts::domain`
  (`Alert`/`AlertLevel`/`Delivery` + a `load_alerts` reading the repo's real `alerts.json`).
- `src/routes/+page.svelte` calls `invoke<Alert[]>("list_alerts")` and renders the alerts with
  neutral, non-Alien styling (a stand-in for shadcn-svelte).

## Validated here (compile-only; GUI needs a display)
- Frontend: `npm run build` → SvelteKit/adapter-static built clean.
- Backend: `cargo check` (src-tauri) → Tauri 2.11 + the command compile clean.

## Run it (on a machine with a display)
```bash
cd web-spike-0a
npm install
npm run tauri dev      # opens a window listing Bishop's alerts, served by Rust over IPC
```
The window lists the alerts from the repo-root `alerts.json` (newest first), each with a
level badge + source/room/time + delivery count — all produced by the Rust command.

## Notes / what this is NOT yet
- The alert types here are a **spike copy**. Phase 1 extracts the real **`crates/core`** shared
  by BOTH the existing ratatui app and `src-tauri`, so the logic is reused verbatim.
- No shadcn-svelte yet. To add it (Tailwind v4 + components):
  ```bash
  npx shadcn-svelte@latest init      # sets up Tailwind + theme tokens
  npx shadcn-svelte@latest add button card badge table
  ```
- **0c (Threlte 3D floor-plan)** is not done yet.

---

## Phase-0b — live video harness (the real go/no-go)

A **go2rtc** sidecar (RTSP→WebRTC, H.264 passthrough, sub-second) feeds a `<video>` player in
the webview — the least-broken cross-platform path for live camera in Tauri.

Pieces:
- `src-tauri/src/lib.rs::spawn_go2rtc()` — starts the sidecar on app launch (best-effort; logs
  if the binary is missing). *Spike shortcut via `std::process`; production = a Tauri externalBin
  sidecar.*
- `src-tauri/go2rtc.yaml` — point `camera:` at your RTSP URL.
- `src-tauri/binaries/DOWNLOAD.md` — one-liners to fetch the go2rtc binary per OS (the agent
  was not allowed to download/run a third-party binary for you — do this step yourself).
- `src/routes/live/+page.svelte` — embeds go2rtc's WebRTC player (`localhost:1984`).

**Validated here:** frontend build + `cargo check` both clean. **NOT validated:** the actual
stream — needs a real camera, a display, and the three OS webviews.

### Run + the cross-OS test (do this on each target OS)
```bash
# 1. edit src-tauri/go2rtc.yaml  → your RTSP URL
# 2. fetch the binary → src-tauri/binaries/go2rtc   (see binaries/DOWNLOAD.md), chmod +x
# 3. cd web-spike-0a && npm run tauri dev   → open the "Live camera" page
```
| OS / webview | Expectation | Why it matters |
|---|---|---|
| **macOS / WKWebView** | should play | baseline |
| **Windows / WebView2** | should play; verify on a **clean box** | proprietary H.264 may be absent |
| **Linux / WebKitGTK (X11)** | **most likely to fail** | WebRTC often OFF by default — the key risk |

If Linux fails, the fallbacks (in order): build/enable WebKitGTK WebRTC; or switch the player to
**MSE/fMP4** (go2rtc also serves that) instead of WebRTC; or last resort, ffmpeg→JPEG-over-IPC for
a low-fps preview. **This OS matrix is the decision point for the whole migration's live-video story.**

