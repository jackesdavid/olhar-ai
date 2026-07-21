# Tuya smart-home integration — Implementation Plan

**Board card:** Tuya integration — provider + workflow nodes to control devices (In Progress)
**Branch:** `feat/tuya-integration`
**Goal:** Add Tuya as a third-party provider so workflows can update/control smart-home devices
(switches, plugs, lights) linked to the tenant's Tuya / Smart Life account.

## Approach (locked, validated end-to-end against real hardware)

- **Credentials are operator-level** — one Sentinel Tuya Cloud project (`TUYA_ACCESS_ID` /
  `TUYA_ACCESS_KEY` / `TUYA_ENDPOINT` env, Western America DC). NOT per-tenant like Telegram.
- **Account link = QR "Tuya App Account Authorization"** in the Tuya console (white-label Izy
  Smart does NOT link; the official **Smart Life** app does). Self-service pairing is deferred.
- **Devices picked by name** in the node (dropdown), stored by stable `device_id` — mirrors the
  `GetCameraByName` pattern. Never expose the raw id.
- Proven with an Intelbras EWS 1002 2-gang switch ("Entrada", `switch_1`/`switch_2`): token,
  list, status read, and command write all work (HMAC-SHA256 signing).

## Phase 1 — app-core `tuya` module (done)
- `crates/app-core/src/tuya/{mod,domain,client,repo}.rs`: `TuyaConfig::from_env`, `TuyaDevice`,
  `TuyaFunction`; `client.rs` HMAC-SHA256 signed `ureq` (token/list/status/command); `repo.rs`
  (`load`/`find_selector`/`upsert_many`).
- Migration `crates/app-core/migrations/0062_tuya_devices.sql` (per-tenant, credential-free).

## Phase 2 — workflow nodes (done)
- Two `NodeKind` (`ALL: 80 → 82`), category **"Smart Home"**: `TuyaControl` + `TuyaReadStatus`,
  config-only (device/code/value dropdowns), exec in `workflows/node/tuya.rs` — resolves the
  device via `sys.db` + `tuya::repo::find_selector`, coerces `value` by the point's Tuya type.

## Phase 3 — control plane (done)
- `sentinel-control/src/domain/tuya.rs`: `GET /tuya/devices`, `POST /tuya/sync`, `GET /tuya/provider`.
- RBAC `tuya.read` added to `rbac.rs` PERMISSIONS (seeded to Admin on boot).

## Phase 4 — frontend (done)
- `lib/data.ts` + `lib/types.ts`: `listTuyaDevices`/`syncTuyaDevices`/`getTuyaProvider`, `TuyaDevice`.
- Editor: `tuya_device`/`tuya_code` dropdowns + **adaptive `value` widget** (Boolean→switch,
  Enum→select, Integer→number).
- **Settings → Integrations** page + nav link gated on `tuya.read`.

## Blast radius (existing production tenants)
Everything is **additive** — no existing node slug/label/port/config changed, so running graphs
execute identically (slug round-trip test passes). Tuya nodes only run if explicitly added; with
env unset `from_env()` returns `None` and only errors if a Tuya node runs. `tuya.read` seeds
additively. Frontend degrades gracefully (`listTuyaDevices` is `.catch(() => [])`).
**The only outage risk is migration 0062** — it runs on `Db::connect` at control + flow boot; a
bad migration = boot retry loop = outage for ALL tenants. It's `CREATE TABLE IF NOT EXISTS` only
(idempotent, no ALTER/DROP/backfill), so control/flow apply order doesn't matter.

## Pending before deploy
1. **Validate migration 0062 on a live/local Postgres** (`task db-up`) — must, per project rule.
2. Set `TUYA_ACCESS_ID` / `TUYA_ACCESS_KEY` / `TUYA_ENDPOINT` on the VPS.
3. End-to-end in-app test: sync → build workflow → toggle "Entrada".
4. Ship via CI (push `main` → deploy.yml); redeploy control + flow + app. Rollback = revert image.

## Out of scope (v1)
- Self-service QR pairing for other tenants (v1 = the Default tenant only).
- Token caching (fetch-per-operation), wiring a dynamic `value` input, unlink/remove devices.
