# Spec: 014 — Web migration: Devices & discovery

> **Status: DONE (code) — web migration unit (M1).** Part of the parallel Tauri/SvelteKit
> build (`docs/web-migration-plan.md`). The ratatui `src/` is frozen; logic is COPIED into
> `app/crates/core`. `cargo check` + `npm run build` clean; `src/` verified untouched. The
> ONVIF network commands (discover/snapshot/test) compile and are runtime-verified on a real
> camera (no camera in CI); the device-registry CRUD + UI are offline-complete.

## Goal

The operator discovers ONVIF cameras and manages a saved device registry in the new web app.

## User Value

The foundational camera feature — every other surveillance feature (live, snapshots, alerts,
map) depends on the device registry. First real migrated screen.

## Requirements

- Copy `src/onvif/*`, `src/scan` Device + persistence, and `src/device/actions.rs` into
  `app/crates/core` (drop the ratatui `ScanState`/sonar; persistence path-parameterized).
- Tauri commands: `list_devices`, `save_device`, `delete_device`, `rename_device`,
  `discover_devices` (ONVIF), `capture_snapshot` (→ base64), `test_credentials`.
- Devices stored as JSON under the OS app-data dir (not the ratatui app's `devices.json`).
- SvelteKit screen: device list (name/ip/model, rename/delete) + a scan action that lists
  discovered devices to add + a device detail (creds, test, capture snapshot preview).

## Acceptance Criteria

- [ ] `list/save/delete/rename_device` round-trip the registry (app-data JSON), shown in the UI.
- [ ] `discover_devices` runs ONVIF WS-Discovery off the UI thread and returns found devices.
- [ ] `capture_snapshot` returns a JPEG (base64) the UI displays; `test_credentials` reports ok/err.
- [ ] `cargo check` (app workspace) + `npm run build` clean; ratatui `src/` untouched.

## Dependencies

- M0 foundation (app shell, crates/core, commands pattern).

## Risks

- ONVIF/discovery/snapshot need real cameras → copied verbatim, verified at runtime (like the
  ratatui app). Offline-testable: device registry CRUD + the UI.
- Discovery is UDP multicast — run on a background thread; macOS Local-Network prompt may gate it.

## Notes

Out of scope here: live RTSP (015). The snapshot is a still JPEG over HTTP/ONVIF.
