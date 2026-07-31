# Web migration — feature-by-feature plan

> Branch `new-ui`. Executes the recommended stack from `web-stack-feasibility.md`:
> **Tauri 2 (Rust backend) + SvelteKit-static + shadcn-svelte + Threlte**. Built in parallel
> to the ratatui app, feature by feature. Updated 2026-06-04.

## Ground rules

1. **The ratatui app (`src/`, root `Cargo.toml`) is FROZEN.** We never modify it. It keeps
   working against its own data files until the web app reaches parity, then it's retired.
2. **Rust logic is COPIED, not shared.** The new app has its own `app/crates/core` copied from
   Bishop's modules. (A shared crate would force edits to `src/` — off the table per rule 1.)
   Accept the temporary duplication; it ends when ratatui is retired.
3. **Keep the module simplicity.** Copy each module's `domain`/`actions` as-is; only the *views*
   (ratatui) are dropped and rebuilt in Svelte. The IO logic is reused verbatim.
4. **One feature = one migration unit = one SDD spec** (`ai/specs/014…`): copy module → add
   Tauri commands → build the SvelteKit screen(s) → wire IPC → verify. Ship/commit per unit.
5. **Rebrand as we go.** Drop the Alien / Weyland-Yutani / MU-TH-UR naming + aesthetic for a
   neutral, original identity (shadcn provides the look) — done in the shell milestone.

## New app layout (a cargo workspace, independent of the root crate)

```
app/                              # the new desktop app (parallel to src/)
├── crates/core/                  # COPIED Bishop logic, UI-agnostic, pure Rust
│   agent · onvif · scan/device · alerts · notification · voice · listen ·
│   audio · config · workflows::{domain,node,engine} · map::domain · persistence
├── src-tauri/                    # Tauri shell
│   commands/                     # #[tauri::command] thin wrappers → crates/core
│   channels: scan results, run logs, alert/stream status, chat stream
├── src/                          # SvelteKit (CSR/SPA, Svelte 5, TS)
│   lib/ui/                       # shadcn-svelte components (copied-in, MIT)
│   lib/ipc/                      # typed invoke() wrappers
│   lib/three/                    # Threlte scenes
│   routes/                       # one folder per feature screen
│   app shell: sidebar nav + header + theme
└── static/ , assets/
```
> The committed `web-spike-0a/` seeds this (its working alerts/map/live/Threlte wiring lifts
> straight in). `app/` is the productionized version with the `crates/core` split + shadcn.

## Migration units (ordered by dependency + value)

| # | Unit (→ spec) | Copy from `src/` | Tauri commands | SvelteKit screen | Notes |
|---|---|---|---|---|---|
| **M0** | **Foundation / shell** | `config` | `get/set_settings` | sidebar nav, header, theme, settings | shadcn + Tailwind v4 setup; rebrand; IPC layer; the `app/` workspace + `crates/core` skeleton |
| **014** | **Devices & discovery** | `scan`, `onvif`, `device` | `list/discover/save/delete/rename_device`, `capture_snapshot`, `test_credentials` | Devices list + Device detail (snapshot) | foundational — most features need cameras. WS-Discovery → channel |
| **016** | **Alerts** | `alerts` | `list/create/deliver/delete_alert`, `alert_stats` | Alerts dashboard (shadcn table/cards) | already spiked (0a) — lift + polish |
| **017** | **Notifications / Beacons** | `notification` | `list/pair/test/toggle/rename_beacon`, pairing poll → channel | Beacons screen | Telegram dispatch stays in backend |
| **018** | **Map (3D)** | `map::domain` | `load/save_map`, `assign_camera` | 3D floor-plan editor (Threlte) | productionize 0c: rooms + camera placement + camera→live link |
| **019** | **Chat / Bishop (agent)** | `agent` (client/tools/vision) | `send_message` (stream via channel), tool exec | Chat screen | tool-use over the migrated commands; vision on snapshots |
| **020** | **Workflows** | `workflows::{domain,node,engine}` | `load/save_graphs`, `run_graph`, run-log channel | Node-graph editor (Svelte Flow) | engine runs backend; UI = the shipped ratatui editor's UX |
| **021** | **Voice / TTS + STT** | `voice`, `listen`, `audio` | `speak`, `start/stop_listen` | settings/controls + transcript | keep `rodio`/`cpal` native (per feasibility) |
| **022** | **Boot / about / branding** | — | — | splash + about | finalize the neutral brand; retire ratatui |

## Milestones

- **M0 — Foundation** (do first): `app/` workspace, `crates/core` skeleton + `config` copied,
  shadcn-svelte + Tailwind, app shell (nav/header/theme), typed IPC layer, neutral branding.
- **M1 — Core surveillance loop:** 014 Devices → 015 Live → 016 Alerts → 017 Notifications.
  (Delivers the sellable core: see cameras, watch live, raise + deliver alerts.)
- **M2 — Spatial + AI:** 018 Map3D → 019 Chat → 020 Workflows.
- **M3 — Finish:** 021 Voice/STT → 022 branding → retire ratatui.

## Per-unit checklist (the SDD loop, applied)

For each unit: `spec.md` (goal/AC) → copy module to `crates/core` → `architecture.md` (commands +
screen + channels) → implement (commands, then screen) → verify (`cargo check` + `npm run build`
+ run the window) → commit. `src/` is never touched.

## Open decisions (confirm before M0)
- App dir name: `app/` (recommended) vs a product-named dir.
- Start point: **M0 foundation first** (recommended) vs jump to a feature.
- Track migration units as SDD specs `014…` (recommended) vs a lighter checklist.
