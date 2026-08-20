# Architecture & services

Sentinel is a cloud SaaS plus one on-prem appliance per site. Cameras are LAN-bound
(ONVIF + RTSP), so the **edge** is mandatory however cloud the rest becomes.

## Services & ports

| Service | Port | Role |
|---|---|---|
| `sentinel-control` | 8790 | Control plane: auth + RBAC + JWT sessions (+ Google OAuth), tenants, the camera config the edge pulls, the box registry, workflow CRUD (proxies flow), the event registry, run-history + box-log relay. Owns Postgres + migrations. Public. |
| `sentinel-flow` | 8787 | Always-on workflow engine: trigger poller + run executor. **Multi-tenant** — one pod runs every tenant's graphs, keyed `(tenant, graph_id)`. Publishes run-progress events to Redis + consumes injected events (edge motion). |
| `sentinel-app` | — | The SvelteKit web UI. Calls control (`API_BASE`) for everything; gateway (`GATEWAY_BASE`) for live. |
| `sentinel-postgres` | 5432 | Shared Postgres (+ pgvector for face embeddings). |
| `sentinel-redis` | 6379 (internal) | Shared event fabric (pub/sub, no persistence): flow's run-progress events (`events:<tenant>`, the SSE relay's source) + edge motion injected into the engine (`flow:inject`). `REDIS_URL`. |
| `sentinel-ai` | — | Face detect/embed sidecar (InsightFace, GPU/CPU). **Cloud = RunPod Serverless on an RTX A4500** (flex / scale-to-zero); `AI_SERVICE_URL`. |

agent/vision, `ports`, `db`) linked by control, flow, and edge. The frontend `lib/data.ts`
is the single API layer (`api.*` typed client). **`sentinel-edge` is NOT in the cargo
workspace** — it links a native dep (OpenCV) and ships independently, so its native deps never
enter the cloud image build; it's built from its own dir (`cd sentinel-edge && cargo build`).

## Data flow

**Edge ⇄ control (over the public internet — the edge dials OUT, no inbound):**
- `GET /config/cameras?wait=25` — long-poll for the tenant's camera list; control wakes it
  via `AppState::bump` (a `Notify`) the instant the web edits a camera. Health headers
  `store::touch_box_by_token` stamps `tenant_boxes`.
- `POST /config/discovered` — ONVIF sweep (every 5 min).
- `POST /config/device-moved` — cameras whose IP changed (reconciled by stable ONVIF id).
- `POST /config/streaming` — IPs the edge has a resolved stream for (drives the Devices dot).
- `POST /config/motion` — a normalized motion observation `{camera_ip, score, area, blobs, bbox}`
  from the edge's MOG2 watcher (box-token → tenant). Control publishes it to Redis `flow:inject`.
- `GET /config/commands` — pending `rescan` (tenant) / `restart` (box) commands, consumed.

**Event fabric (Redis pub/sub):**
- Flow publishes every run-progress event (`workflow-run`/`-node`/`-log`/`-activity`/`-image`/…)
  to `events:<tenant>`. Control's SSE relay (`/workflows/events`) subscribes to that channel, so
  it sees EVERY flow pod's events (multi-pod ready) and streams the tenant's to the browser.
- `flow:inject` carries externally-originated events; flow's subscriber turns each into
  `engine::fire_event(System, name, payload)` → fans out to that tenant's `OnEvent(name)` graphs.

**Edge motion → workflow:** camera motion → edge MOG2 watcher → `POST /config/motion` → control
publishes to `flow:inject` → flow fires the `motion` System event (typed payload camera/score/
area/blobs/bbox) → `OnEvent("motion")` handler graphs run. The edge is a pure sensor; per-graph
sensitivity is applied in the cloud (`If score >= …`). `motion` is a **system event** (code
catalog, available to every tenant — see [workflows ref](workflows.md)).

**Browser ⇄ control** (`API_BASE`, JWT bearer): everything except live video.

the gateway resolves the tenant's box host via control `/internal/box-upstream` (cached 30s),
then bridges to `wss://<box-host>/api/ws` where the box is directly addressable, and over the
edge's own outbound connection otherwise. Snapshots/frames go the same way.

`edge_base(tenant)` = the tenant's box host and fetches `/api/frame.jpeg?src=cam_<ip>`.

## Multi-tenancy

`tenant_id` is on every domain table (migration 0011). Control derives the tenant from the
user's JWT; the edge derives it from its `BOX_TOKEN` (sha256 → `tenant_boxes`); flow is
inherently multi-tenant (loads all tenants' graphs, keys everything by `(tenant, …)`). The
SSE relays filter strictly by the caller's tenant.

## How the cloud reaches LAN cameras

The edge is **outbound-only** (card #144). It dials control over HTTPS and holds one outbound SSE
command stream; the cloud enqueues commands (frames, probes, recordings, live) and the box picks
them up on that stream. The box reports its host on the heartbeat (`X-Box-Host` →
`tenant_boxes.magicdns_host`), which `box_host_for_tenant` returns — used only where the edge is
directly addressable, i.e. dev. **A direct HTTP call from a cloud service to a deployed box reaches
nothing.** The private-mesh era was removed from the code in #199.

## Key persisted state (Postgres, `crates/app-core/migrations`)

- `devices` (PK `(tenant_id, ip)`, carries stable ONVIF `id`), `configured_cameras`,
  `streaming_cameras`, `discovered_cameras`.
- `tenant_boxes` (box registry: token hash, magicdns_host, version, last_seen + health:
- `workflows` (graph JSONB), `workflow_runs` + `workflow_run_logs` (run history),
  `workflow_cron_state`, `tenant_events` + `tenant_event_categories` (event registry),
  `box_logs` (edge log tail).
- `tenants`, users, roles, permissions (RBAC); `alerts`; `notification_targets`; `map_rooms`.
