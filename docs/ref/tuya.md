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

**The OEM licence has a number, established 2026-08-09 so nobody re-researches it:** the SmartLife
App SDK costs **USD 5 000 the first year and USD 2 000/yr to renew**, and the Official edition is
MANDATORY to ship on an app store — the free Development edition caps at 100 registered users per
data centre and is explicitly non-commercial. Rejected on cost: an annual floor paid before the
first customer, for something BYO already does. (Programmatic sign-up is not the blocker —
`registerAnonymousWithCountryCode:` creates an account with no verification code — and the iOS SDK
ships via CocoaPods. The money was the blocker.)

**Conclusion:** BYO-project sidesteps all of it — the tenant links their OWN account to their OWN
project (self-consent), no OEM, no hacks, ToS-clean.

## `local_key` — where it comes from (researched 2026-08-13, do not re-research)

Question asked: *can our own app discover a device's key, in any way?* **No — and it does not
matter, because the credentials we already hold can fetch it.**

- The `local_key` is **issued by the Tuya cloud at pairing/activation**. It is not derivable from
  the device and the device never transmits it: in protocol 3.4/3.5 it only validates the handshake
  and derives a per-session key (nonce XOR, AES-GCM, HMAC-SHA256). Sniffing the LAN yields the
  session key at best, never the static one.
- It **changes every time the device is re-paired**, so any key captured out-of-band goes stale the
  first time a customer re-adds a device.
- **Our own app doing the pairing would not help.** The app must first obtain a pairing token from
  the cloud, and the key comes back afterwards *through* the cloud device list. That is the OEM SDK
  path — the same USD 5 000/yr wall documented above, reached from a different direction.
- **What DOES work, with what we already have:** `GET /v1.0/users/{uid}/devices` returns `local_key`
  per device. We already store the tenant's linked app-account uids (`repo::linked_uids`) and their
  BYO project credentials, so local control (#133) needs **no new credential model** — only a
  different listing call than the `/v1.0/iot-01/associated-users/devices` sync uses today. It does
  need the Device Management / IoT Core authorisation on the project, and a re-sync after any
  re-pair.

Sources: [Tuya "Get User's Device List"](https://developer.tuya.com/en/docs/cloud/cacc9c4989?id=Ka7kk03zdecl4),
[tinytuya PROTOCOL.md](https://github.com/jasonacox/tinytuya/blob/master/PROTOCOL.md),
[tinytuya #111](https://github.com/jasonacox/tinytuya/discussions/111),
[localtuya README](https://github.com/rospogrigio/localtuya).

## Thing model vs the v1 API family (learned the hard way, 2026-08-14)

Not every Tuya product is described by the same API family, and picking the wrong one looks
exactly like a broken device.

A gate module (category `ckmkzq`, product "Module Wifi", model "Garen Mobile") answered **every**
v1.0/v1.1 metadata and command endpoint with an error while working perfectly in the Smart Life
app:

| endpoint | answer |
|---|---|
| `/v1.0/devices/{id}/functions` | `2009 not support this device` |
| `/v1.1/devices/{id}/specifications` | `2009` |
| `/v1.0/iot-03/devices/{id}/specification` | OK, but only `{"category":"ckmkzq"}` |
| `/v1.0/devices/{id}/status` | `2003 function not support` |
| `/v1.0/devices/{id}/commands` | `2008 command or value not support` |
| **`/v2.0/cloud/thing/{id}/model`** | **OK — every property, with real names and `accessMode`** |
| **`/v2.0/cloud/thing/{id}/shadow/properties`** | **OK — code, dp_id, type, current value** |
| **`/v2.0/cloud/thing/{id}/shadow/properties/issue`** | **OK — writes** |

So `client.rs` tries the **thing model first** for metadata, status and commands, falling back to
v1 (which is what the already-working devices use, and must not be risked).

Two details that cost time:

- **`/model` is preferred over `/shadow/properties`** for the catalog. Only the model carries the
  real names ("Portão 1", not `gate1`), `accessMode`, and enum ranges. Dropping `accessMode` means
  the editor offers read-only points — limit switches — in the Control node, and writing one fails
  at run time. Treat a missing `access` as writable: the v1 sources describe only functions.
- **`result.model` is a JSON STRING**, not an object.

**The method, not just the answer:** two fixes were deduced from documentation and logs, and both
were wrong, because they assumed which API family the device belonged to. Only probing every
endpoint against the real device settled it. When a Tuya device misbehaves, probe first — and probe
the RIGHT tenant's project (`tuya_config` has one row per tenant; a `LIMIT 1` reads someone else's
and produces a confidently wrong diagnosis).

## Gate boards: the command point is not the state point (2026-08-15)

A "House Gate" workflow always asked "Fechar o portão?", whatever the gate was doing. The cause is
worth generalising, because any pulse-driven device has it.

**The graph read `gate1` — the COMMAND point — and branched on it.** On a gate board, writing
`gate1 = true` is a momentary pulse that runs the motor; the stored value is just *the last thing
written*, not where the gate is. After the first approval it stayed `true` for ever, so the branch
froze. Confirmed from `device_gate_requests`: two runs said "Abrir", then an approval at 12:34:49,
then "Fechar" on every run after it.

**The state lives in the limit switches.** On this board (Garen, `ckmkzq`):

| code | reads |
|---|---|
| `gate1_fca` | **`True` when the gate is CLOSED**, `False` when open |
| `gate1_status` | `2` at rest, `1` while travelling |
| `gate1_fcf`, `gate2_*`, `social_gate_sensor` | frozen since first report — **not wired** on this install |

Note `gate1_fca` is **inverted relative to its name**. "FCA" reads as *fim de curso aberto*, and it
is `True` when CLOSED. Do not infer polarity from a data point's name — confirm it against the
physical state, twice, in both positions.

**How this was got wrong, so it is not repeated.** Two diagnoses were built on unlabelled captures:
the transitions were recorded, but the gate's actual position at each end was *assumed* rather than
asked. Both assumptions were wrong, and the second produced a confident safety warning that did not
apply. A device capture is only evidence if someone looking at the device says what state it was
in — the timestamps prove a change happened, never which way.

## The device console (#191, 2026-08-18)

`/settings/integrations?tab=tuya` expands a device into a **console**: every data point readable and
individually sendable, so a person proves what a device does before a workflow depends on it.

Three things make it work, and all three were already in the data:

- **`values` and `access`** are cached per function by `tuya/client.rs` from the v2.0 thing model.
  The page used to map every point down to `{code, type}` and discard both — which is why a
  read-only limit switch looked identical to a command. With them restored, an enum renders its
  range, an integer its bounds, and **`access: "ro"` renders no control at all**.
- **`POST /tuya/devices/:id/test` already writes.** Given `{code, value}` it calls `send_command`;
  given neither it calls `get_status`. No route was added for the console. The name is now a
  misnomer — a "test" that opens a gate — but a second endpoint doing the same thing would be worse.
- **Sorting by the first integer in the code** groups a channel: `gate1`, `gate1_fca`, `gate1_fcf`,
  `gate1_status`, then `gate2_*`, then unnumbered device-wide points. Channel first, because a
  command and its limit switch are the pair that proves a write landed.

**Security boundary.** The console is a browser writing to physical hardware, and the two acts it
performs need different rights (**#192**, done):

| Act | Permission |
|---|---|
| Read a device's status | `tuya.read` |
| Link an account, edit labels | `tuya.write` |
| **Send a command — open a gate, switch a relay** | **`tuya.control`** |

`POST /tuya/devices/:id/test` decides which to require from the PAYLOAD, not the route: with
`code` + `value` it writes and needs `tuya.control`; without them it only reads. Checking the route
name would be useless — it is called "test" and describes neither act.

Every send also passes through a confirm naming device, control and value, and the Send button is
hidden from a user without `tuya.control` rather than offered and then 403'd.

**Existing tenants need no migration:** `seed.rs` upserts the catalog on boot and grants every
action to each tenant's system Admin role. A CUSTOM role previously given `tuya.write` loses
command-sending — which is the entire point, because that grant was never meant to include opening
doors.

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
- **Done + LIVE.** The per-control console (#191) — read all, send one, confirmed, channel-grouped,
  read-only points inert. Validated in production on the Garen gate board.
- **Backlog:** #192 (split `tuya.control` out of `tuya.write`), #132 (filter non-controllable String
  data points from the dropdown). OEM self-service app = deferred product decision.
- **Closed, not done:** #133 (local control via the edge) — closed 2026-08-17 by the owner. Cloud
  control is good enough in practice; the map and its decision tickets stay as research.
