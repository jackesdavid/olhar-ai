# Tuya smart-home integration

Control smart-home devices (switches, plugs, lights) **from workflows**. Tuya is a third-party
provider: one operator-level Tuya Cloud project + per-tenant device catalog + two workflow nodes.
Shipped 2026-07-21 (card #129 / GitHub issue #9, Done).

## Model (the decisions)

- **Credentials are operator-level** — ONE Sentinel Tuya Cloud project. Env vars (like
  `ANTHROPIC_API_KEY`), NOT per-tenant like Telegram: `TUYA_ACCESS_ID`, `TUYA_ACCESS_KEY`,
  `TUYA_ENDPOINT` (default `https://openapi.tuyaus.com` — Western America DC, validated for BR).
  On the VPS these live in `/ibbf/sentinel.env` (env_file of control + flow).
- **Auth = HMAC-SHA256** per Tuya's "Sign Requests" spec. Token: `GET /v1.0/token?grant_type=1`
  signs `client_id + t + STS`; business calls sign `client_id + access_token + t + STS`, where
  `STS = METHOD\nSHA256(body)\n\n<path>`. Token fetched per-operation (no cache in v1).
- **Device linking = QR "Tuya App Account Authorization"** (Tuya console → Devices → Link Tuya App
  Account). One project; each end-user links their app account by scanning a QR → their devices
  appear under `GET /v1.0/iot-01/associated-users/devices`. **The white-label Izy Smart app does
  NOT link via that QR** (separate schema) — the official **Smart Life** app does. The
  user/password login (`/v1.0/iot-03/users/login`) is blocked for white-label (private schema →
  `1106 permission deny`). v1 links via the console manually (Default tenant only).
- **Devices picked by NAME** in the node (dropdown), stored by the stable `device_id`, never the
  raw id — mirrors `GetCameraByName`. Renaming in Smart Life doesn't re-point a node.

## File layout

| Path | What |
|---|---|
| `crates/app-core/src/tuya/domain.rs` | `TuyaConfig::from_env`, `TuyaDevice` (credential-free row), `TuyaFunction` (type metadata) |
| `crates/app-core/src/tuya/client.rs` | signed `ureq` calls: `token`, `fetch_devices` (list + per-device functions), `send_command`, `get_status` |
| `crates/app-core/src/tuya/repo.rs` | `tuya_devices` table: `load` / `find_selector` / `upsert_many` |
| `crates/app-core/migrations/0066_tuya_devices.sql` | per-tenant device catalog (see migration note below) |
| `crates/app-core/src/workflows/node/tuya.rs` | node execution: resolve device via `sys.db`, coerce `value` by function type, call the client |
| `sentinel-control/src/domain/tuya.rs` | `GET /tuya/devices`, `POST /tuya/sync`, `GET /tuya/provider`; RBAC `tuya.read` |
| `sentinel-app/src/routes/settings/integrations/+page.svelte` | Integrations page (Connected badge, device list, Refresh) |
| `sentinel-app/src/routes/workflows/[[id]]/+page.svelte` | editor dropdowns (`tuya_device`, `tuya_code`) + adaptive `value` widget |

## The two nodes (category "Smart Home", `NodeKind::ALL`)

- **Control Tuya Device** (`tuya_control`) — config `tuya_device` (dropdown), `tuya_code` (e.g.
  `switch_1`), `value` (typed per the point: Boolean→switch, Enum→select, Integer→number). Sends
  one command. Outputs `ok`, `response`. Exec passes through.
- **Read Tuya Status** (`tuya_read_status`) — config `tuya_device`, `tuya_code`. Outputs `value`
  (Text), `on` (Bool convenience), `raw` (full status JSON).

Both config-only (no data input ports). `value` coercion (bool/int/enum/string) is in
`node/tuya.rs`; the editor's adaptive widget reads types from the device's `functions_json`.

## ⚠️ Migration numbering — the outage lesson

The tuya migration was FIRST written as `0062` trusting a **stale `CLAUDE.md`** note ("next 0062").
But the repo already had `0060`–`0065` (faces pipeline, box hwaccel, stream mode). `0062` collided
with `0062_faces_labeling` → two version-62 migrations in the binary → sqlx checksum mismatch →
**control + flow boot-loop (~12 min outage; gateway/live video unaffected)**. Fixed by renumbering
to **`0066`** + redeploy. **ALWAYS `ls crates/app-core/migrations/` for the real next number** —
never trust the CLAUDE.md note.

## Build status

- **Done + LIVE in production** (merged `feat/tuya-integration` → `main`, deployed via CI). Verified
  in prod: Integrations shows **Connected** + "Entrada" (Intelbras EWS 1002) **Online**; the
  Control Tuya Device node toggled the physical relay end-to-end.
- **Not done (future):** self-service QR pairing for other tenants (v1 = Default tenant, console
  link only); token caching; wiring a dynamic `value` input; unlink/remove a device; per-tenant
  device→tenant mapping (the `tuya_uid` column is the hook).

## Test in-app

Settings → Integrations → **Refresh devices** (lists linked devices) → build a workflow with
**Control Tuya Device** (device, `switch_1`, On) → Run → relay fires. **Read Tuya Status** confirms.
Standalone prototype scripts (`tuya_probe.py` / `tuya_control.py`, manual HMAC signing) live in the
session scratchpad.
