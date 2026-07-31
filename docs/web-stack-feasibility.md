# Feasibility & Migration Plan — Bishop → Tauri (Rust core) + Svelte web UI

> Status: **Feasibility study (branch `new-ui`)**, updated 2026-06-04 after a commercial-direction
> pivot. Decision artifact, not yet a committed migration. Cited against current (2026) sources.

## TL;DR

Migrate Bishop to a **Tauri 2** desktop app that **keeps the entire Rust codebase as the
backend** and replaces the ratatui UI with a **professional web frontend: SvelteKit (static/CSR)
+ shadcn-svelte components + Threlte (declarative Three.js) for the 3D floor-plan.** Drop the
retro "Alien / Weyland-Yutani / MU-TH-UR" CRT aesthetic and naming for a neutral, sellable
design — shadcn-svelte gives this out of the box, and it removes a real trademark/IP risk for a
commercially sold product.

This is **easier and more commercially viable** than the all-Rust (Leptos) variant, and it
**preserves the module simplicity you want to keep**:

- **Backend: keep everything, unchanged.** agent / onvif / alerts / notification / voice / STT /
  audio / workflows-engine / map-data become a `crates/core` library behind `#[tauri::command]`
  wrappers. Reuse is identical no matter which frontend you pick — Tauri commands are
  language-agnostic IPC. [arch](https://v2.tauri.app/concept/architecture/)
- **Frontend: a rewrite, but the well-trodden one.** SvelteKit is a first-party Tauri frontend;
  **shadcn-svelte** ships 70+ professional, accessible, MIT, copy-in components (data tables,
  dialogs, forms, command palette); **Threlte** makes Three.js native (no WASM↔JS boundary).
- **3D gets much easier.** Threlte (`useGltf`, `<OrbitControls>`, interactivity/raycasting,
  postprocessing) is declarative Three.js. The Leptos route would hand-write wasm-bindgen glue
  for every loader/control/shader.

**Verdict: 🟢 GREEN. Recommend proceeding** — still gate on one Phase-0 media spike (the only
real risk, and it's framework-independent).

---

## Why Svelte over the all-Rust (Leptos) variant

| Axis | **SvelteKit + shadcn-svelte + Threlte** (recommended) | Leptos + Three.js (all-Rust alt) |
|---|---|---|
| Maturity / Tauri support | First-party documented path, huge ecosystem | Niche; community-maintained glue |
| Components | shadcn-svelte: 70+, professional, MIT, runes-native | thin widget ecosystem — build your own |
| 3D | **Threlte: declarative Three.js, zero FFI** | wasm-bindgen bindings for every loader/control |
| Dev loop | Vite instant HMR | recompiles Rust→WASM on each change |
| Hiring / docs | the whole JS/TS market | small Rust-frontend pool |
| Trade-off | adds a Node/TS/pnpm toolchain + 2nd language | stays all-Rust, but slower & sparser |
| **Rust backend reuse** | **identical** | **identical** |

Sources: SvelteKit-static for Tauri [tauri/sveltekit](https://v2.tauri.app/start/frontend/sveltekit/) ·
shadcn-svelte (Svelte 5 runes, Tailwind v4, MIT) [shadcn-svelte](https://www.shadcn-svelte.com/docs/components) ·
Threlte (`@threlte/extras` released Jun 2026; GLTF/OrbitControls/interactivity/postprocessing)
[threlte](https://threlte.xyz/) · Leptos niche/thin ecosystem
[thenewstack](https://thenewstack.io/want-a-web-framework-for-rust-not-javascript-try-leptos/).

---

## Per-area feasibility

| Area | Verdict | Summary |
|---|---|---|
| **Reusing the Rust core** (agent/onvif/alerts/notification/voice/STT/audio/workflows-engine/map-data) | 🟢 Green | Copy-paste into `crates/core`, expose as commands. The simplicity you love is preserved. Wrap blocking `ureq` in `spawn_blocking`; `tokio::sync::Mutex` for state across `.await`. |
| **Tauri 2 + SvelteKit (static/CSR)** | 🟢 Green | `@sveltejs/adapter-static` + `ssr=false`; first-party, Vite HMR, mainstream. (SvelteKit is a documented 3-line manual config, not a one-command scaffold anymore.) [tauri](https://v2.tauri.app/start/frontend/sveltekit/) |
| **shadcn-svelte UI** | 🟢 Green | Professional, neutral, sellable look out of the box; 70+ components; MIT copy-in. Replaces the bespoke CRT chrome. |
| **3D floor-plan (Threlte / Three.js)** | 🟢 Green | GLTF room/camera models, OrbitControls, click-to-select (raycast), postprocessing — declarative, native JS. |
| **Snapshots (HTTP JPEG) + ONVIF (SOAP/WS-Discovery)** | 🟢 Green | Stay in Rust backend; copy-paste. WS-Discovery is UDP multicast → emit results via a Tauri **channel** as devices reply. |
| **Outbound APIs (Claude/ElevenLabs/Telegram)** | 🟢 Green | Keep in Rust commands — avoids CORS, keeps keys out of webview JS. |
| **Mic/STT (`cpal`) & audio/TTS (`rodio`)** | 🟢 Green | Keep both native in the backend → sidesteps the webview `getUserMedia`/codec minefield. |
| **Cross-platform webview media (#1 risk)** | 🟡 Yellow | Linux WebKitGTK WebRTC **off by default**; Windows WebView2 lacks proprietary H.264/AAC by default. **Framework-independent** — same for Svelte or Leptos. [#8426](https://github.com/orgs/tauri-apps/discussions/8426) · [WebView2 #2997](https://github.com/MicrosoftEdge/WebView2Feedback/issues/2997) |

---

## Target architecture

```
bishop/                          (cargo workspace + a SvelteKit app)
├── crates/core/                 ← Bishop's brain, UI-agnostic, pure sync Rust (UNCHANGED)
│   agent · onvif · scan/device · alerts · notification · voice · listen(cpal) ·
│   audio(rodio) · config · workflows::{domain,node,engine} · map::domain · serde_json
├── src-tauri/                   ← thin Tauri shell (Rust)
│   commands.rs  #[tauri::command] → crates/core
│   channels/events: scan results, run logs, alert deltas, stream status
├── src/                         ← SvelteKit frontend (CSR/SPA, TypeScript)
│   routes/+layout.ts            export const ssr=false; prerender=false
│   lib/components/ui/           shadcn-svelte components (copied-in, MIT)
│   lib/ipc/                     typed invoke() wrappers → commands
│   lib/three/                   Threlte scenes (GLTF rooms/cameras, OrbitControls, raycast, CRT-opt post)
│   routes/                      menu · scan · device · chat · map3d · workflows · alerts · beacons · config
├── static/ , assets/3d/*.glb    bundled UI + 3D models
├── svelte.config.js             adapter-static, fallback:'index.html'
├── vite.config.ts , app.css     Tailwind v4 + shadcn theme tokens
└── package.json                 pnpm: @tauri-apps/api, threlte, three, shadcn-svelte
```

**Principle (unchanged):** all capture/decode/audio/API I/O stays **native Rust**; the webview
only renders (WebRTC `<video>`, data via commands/events). This neutralizes the webview-media
risk regardless of frontend.

---

## Module-by-module mapping (current `src/` → target)

| Current module | Target home | Notes |
|---|---|---|
| `agent/`, `onvif/`, `voice/`, `listen/`, `audio/` | **backend (crates/core)** | Copy-paste; commands. Keep `cpal`/`rodio`. |
| `scan/`, `device/`, `alerts/`, `notification/`, `config/` (domain+actions) | **backend** | Logic/stores stay; WS-Discovery → channel. |
| `workflows/{domain,node,engine}` | **backend** | Engine runs server-side; run logs via a channel. |
| `workflows/{handler,view}` | **frontend (Svelte)** | Node-graph editor in Svelte (SVG/canvas, e.g. Svelte Flow); the shipped ratatui editor is the UX spec. |
| `map/{domain}` | **backend** | Room/camera data. |
| `map/{handler,view}` | **frontend (Threlte)** | 3D scene: room/camera GLTF objects + click-to-select. |
| `boot/`, `splash/`, `menu/`, `chat/`, `about/` | **frontend (shadcn-svelte)** | Straight components. |
| `ui/{theme,chrome,image,wireframe}`, `crt/` | **dropped / re-styled** | Replaced by shadcn theme. CRT becomes an *optional* Threlte post-effect, not core. |
| `main.rs` (terminal/CRT routing) | **replaced** | Tauri `Builder` + Vite entry. |

---

## Commercial / branding note (why we're dropping the look)

"Alien", "Weyland-Yutani", "Nostromo", "MU-TH-UR", and that visual/typographic identity are the
**IP of 20th Century Studios / Disney** (the Alien franchise). Shipping that aesthetic, naming, or
logo on a **commercially sold** product invites trademark/copyright exposure — fair-use arguments
are weak for commercial sale.
[ref](https://www.space.com/alien-movies-history-of-weyland-yutani-corporation) Rebrand to a
**neutral, original professional identity** — which shadcn-svelte's clean default design system
provides with no derivative-IP risk. (Keep the *concept* — an AI surveillance console — just not
the franchise dressing or the "BISHOP/MU-TH-UR" naming.)

---

## Hard parts & de-risking (unchanged by the frontend choice)

1. **Webview media inconsistency (the only real risk).** Capture/decode/audio stay in Rust; live
   video via a **receive-only** WebRTC sidecar. **Test on real WebKitGTK Linux (X11) + a clean
   Windows box early.**
2. **Async/blocking discipline.** `spawn_blocking` around `ureq`; `tokio::sync::Mutex` across `.await`.
3. **Two toolchains now** (Rust + Node/TS/pnpm) — honest added cost vs the all-Rust repo today.

---

## Phased migration plan

- **Phase 0 — Spikes (gate the decision). ~2–4 days.**
  - **0a** Tauri 2 + SvelteKit-static shell; extract a `crates/core` slice (e.g. alerts); call one
    command from a shadcn-svelte button. Proves the stack + IPC + reuse.
    Windows**. The make-or-break test.
  - **0c** A Threlte scene loading a GLB room with OrbitControls + click-to-select. Proves 3D.
- **Phase 1 — Extract `crates/core`** (keep ratatui building against it during migration). ~days.
- **Phase 2 — Tauri commands + SvelteKit scaffold + easy screens** (config, alerts, beacons, chat,
  about) with shadcn-svelte. ~1–2 wks.
- **Phase 3 — Media** (snapshots, live via sidecar, mic/TTS/audio in backend). ~1 wk.
- **Phase 4 — 3D map** (Threlte: rooms/cameras, raycast select, camera→live-feed). ~1–2 wks.
- **Phase 5 — Workflows editor** in Svelte (Svelte Flow / canvas). ~1 wk.
- **Phase 6 — Branding, packaging, updater, polish, retire ratatui.** ~1 wk.

**Rough total:** a few days scaffolding + **~5–7 weeks** of UI rebuild (faster than the Leptos
estimate thanks to shadcn-svelte + Threlte), dominated by the screen rewrite + 3D map.

---

## Open decisions

- **Workflows graph UI:** Svelte Flow (JS lib) vs hand-built canvas.
- **CRT effect:** drop entirely, or keep as a subtle optional Threlte post-pass for character.
- **Scope of v1:** all screens web-first, or high-value first (map-3D, live, chat, alerts) with
  the rest following.
- **Incremental vs big-bang:** recommend incremental — ratatui stays alive against `crates/core`
  until web parity, then retire.

## Recommended next step

Run **Phase 0** (the three spikes) on `new-ui`. **0b (media on Linux/Windows) is the real
go/no-go.** If they pass, promote this study to SDD spec `ai/specs/014-web-stack-migration/` and
execute the phases through the loop.

### Sources
Tauri+SvelteKit: https://v2.tauri.app/start/frontend/sveltekit/ · create-tauri-app v3: https://v2.tauri.app/blog/create-tauri-app-version-3-released/ ·
shadcn-svelte: https://www.shadcn-svelte.com/docs/components · https://www.shadcn-svelte.com/docs/migration/svelte-5 ·
Threlte: https://threlte.xyz/ · https://github.com/threlte/threlte ·
Tauri arch/IPC: https://v2.tauri.app/concept/architecture/ · async: https://dev.to/hiyoyok/rust-async-in-tauri-v2-what-tripped-me-up-and-how-i-fixed-it-1662 ·
Branding/IP: https://www.space.com/alien-movies-history-of-weyland-yutani-corporation
