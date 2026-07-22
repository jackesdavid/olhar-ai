# Tuya smart-home integration

Control smart-home devices (switches, plugs, lights) **from workflows**. Two workflow nodes
(Control / Read) + a per-tenant device catalog + a self-service "bring your own Tuya project"
onboarding. Shipped 2026-07-21/22 (cards #129 + #131, Done).

## Credential model — per-tenant "bring your own Tuya project"

**Each tenant connects its OWN Tuya IoT Cloud project** (Settings → Integrations): the tenant
creates a Tuya project, links their own Smart Life account (QR, on their own console — self-consent),
and pastes their **Access ID / Access Secret**. Stored in the per-tenant `tuya_config` table
(write-only to the UI), validated (fetch token) before saving. **Strictly per-tenant — no global
env fallback** (so a tenant never sees another's devices). This is the standard Home Assistant /
LocalTuya / tinytuya pattern. `resolve_config(db, tenant)` (in `tuya::repo`) is the single resolver;
`from_env()` exists but is no longer used for resolution.

- **Auth = HMAC-SHA256** ("Sign Requests"). Token: `GET /v1.0/token?grant_type=1` signs
  `client_id + t + STS`; business calls sign `client_id + access_token + t + STS`, where
  `STS = METHOD\nSHA256(body)\n\n<path>`. Fetched per-operation.
- **Devices picked by NAME** in the node (dropdown), stored by stable `device_id` — mirrors
  `GetCameraByName`. Per-function **friendly labels** (`switch_1 → "Luz Escada"`) live in
  `tuya_devices.function_labels_json`, edited on the Integrations page, **preserved across sync**
  (excluded from `upsert_many`'s ON CONFLICT).

## Why "BYO project" (the self-service options we ruled out)

Self-service in-app linking to ONE Sentinel project is **not possible** without an OEM app:
- **OAuth 2.0** — Tuya console says *"OAuth authorization only supports OEM APPs"*. Blocked for Smart Life.
- **User/password login** (`/v1.0/iot-03/users/login`) — `1106 permission deny` (project not authorized).
- **QR "App Account Authorization"** — works, but the QR is console-generated (no public API for
  non-OEM). Automating the console = scraping the operator's browser session (`x-frame-options: deny`
  kills iframe; SOP + HttpOnly cookies kill child-window capture; captcha kills auto-login) — ToS-gray
  and risks suspending the shared operator account → all tenants down. Rejected.
- **OEM app** — the only sanctioned self-service unlock, but it's a whole published/maintained mobile
  app (users install YOUR app, not Smart Life) + Tuya OEM fees. Deferred (product decision).

**Conclusion:** BYO-project sidesteps all of it — the tenant links their OWN account to their OWN
project (self-consent), no OEM, no hacks, ToS-clean.

## File layout

| Path | What |
|---|---|
| `crates/app-core/src/tuya/domain.rs` | `TuyaConfig` (+`from_env`), `TuyaDevice` (credential-free row, +`function_labels_json`), `TuyaFunction` |
| `crates/app-core/src/tuya/client.rs` | signed `ureq`: `token`, `validate`, `fetch_devices(cfg, allowed_uids)`, `send_command`, `get_status` |
| `crates/app-core/src/tuya/repo.rs` | `tuya_devices` + `tuya_config` + `tuya_account_links`: `load`/`find_selector`/`upsert_many`/`set_function_labels`/`resolve_config`/`load_config`/`save_config`/`delete_config`/`link_account`/`linked_uids` |
| `crates/app-core/migrations/` | `0066_tuya_devices`, `0067_tuya_labels_and_links`, `0068_tuya_config` |
| `crates/app-core/src/workflows/node/tuya.rs` | node exec: resolve creds per-tenant, coerce/typed value |
| `sentinel-control/src/domain/tuya.rs` | `/tuya/devices`, `/tuya/sync`, `/tuya/provider` (`{configured,source}`), `PATCH …/labels`, `PUT/DELETE /tuya/config`; RBAC `tuya.read`/`tuya.write` |
| `sentinel-app/src/routes/settings/integrations/+page.svelte` | connect flow (guided walkthrough + creds form) + device list + label editor + Disconnect |
| `sentinel-app/src/routes/workflows/[[id]]/+page.svelte` | node dropdowns (`tuya_device`/`tuya_code` by name) + dynamic typed `value` input (control) + output (read) |

## The two nodes (category "Smart Home")

- **Control Tuya Device** (`tuya_control`) — `tuya_device` + `tuya_code` dropdowns; `value` is a
  **dynamic input port** typed by the selected data point (Boolean→Bool, Integer→Num, Enum/String→Text)
  — wire a dynamic value, or use the adaptive config-literal widget. Outputs `ok`, `response`.
- **Read Tuya Status** (`tuya_read_status`) — `value` is a **dynamic output port** typed the same way
  (e.g. `countdown_1` → Num), plus `on` (Bool) + `raw` (Text JSON).

The type is baked into `config.value_type` by the editor on code selection (hidden from the panel via
MANAGED_KEYS); `GraphNode::input_ports`/`output_ports` resolve the port from it (like Set Variable).

**Device functions (EWS 1002 example):** `switch_1/2` (Boolean, on/off), `countdown_1/2` (Integer
seconds — auto-off timer), `relay_status`/`light_mode` (Enum, one-time config), `cycle_time`/
`random_time`/`switch_inching` (String, Tuya-encoded timers — not practical from a workflow → #132
to filter them out of the dropdown).

## Local control (future — #133)

The cloud API already returns each device's **`local_key`** (in `associated-users/devices`). With
`id + local_key + LAN ip + protocol version`, the **edge** can control devices **locally** (offline,
fast, no cloud). Cloud gives id+local_key (the WAN ip it returns is useless); the edge scans the LAN
(UDP 6666/6667) for the local ip + version. `local_key` is a secret → lives on the edge (like camera
creds), never the cloud DB. Proven by tinytuya/LocalTuya.

## ⚠️ Migration numbering — the outage lesson

The tuya migration was first written as `0062` trusting a **stale CLAUDE.md** note; it collided with
`0062_faces_labeling` → sqlx checksum mismatch → **control+flow boot-loop (~12 min outage)**. Fixed by
renumbering to `0066`. **ALWAYS `ls crates/app-core/migrations/` for the real next number** — and
check the prod `_sqlx_migrations` max version. Validate every new migration on a live DB before deploy.

## Onboarding runbook (a new tenant)

Tenant: (1) install Smart Life + pair devices; (2) create a Tuya project at **platform.tuya.com** →
Smart Home, pick data center; (3) subscribe IoT Core + Authorization + Smart Home Basic Service
(free trial — can expire, renewable); (4) Devices → Link App Account → scan QR with Smart Life;
(5) copy Access ID/Secret from Overview → paste in Sentinel → Integrations → Connect. The guided
walkthrough is in the Integrations page.

## Build status

- **Done + LIVE.** Per-tenant BYO creds, friendly labels, dynamic typed value in/out — all deployed
  and validated in prod (BYO flow dogfooded end-to-end: connect → sync → toggle relay).
- **Backlog:** #132 (filter non-controllable String data points from the dropdown), #133 (local
  control via edge). OEM self-service app = deferred product decision.
