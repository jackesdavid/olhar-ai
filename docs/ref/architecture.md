# Architecture & services

Sentinel is a cloud SaaS plus one on-prem appliance per site. Cameras are LAN-bound
(ONVIF + RTSP), so the **edge** is mandatory however cloud the rest becomes.

## Services & ports

| Service | Port | Role |
|---|---|---|
| `sentinel-control` | 8790 | Control plane: auth + RBAC + JWT sessions (+ Google OAuth), tenants, the camera config the edge pulls, the box registry, workflow CRUD (proxies flow), the event registry, run-history + box-log relay. Owns Postgres + migrations. Public. |
| `sentinel-flow` | 8787 | Always-on workflow engine: trigger poller + run executor + SSE event bus. **Multi-tenant** — one pod runs every tenant's graphs, keyed `(tenant, graph_id)`. |
| `sentinel-gateway` | 8789 | Public streaming bridge: MSE-over-WebSocket live video + snapshot passthrough, proxied to the tenant's edge go2rtc over the tailnet. Gated by the `live.view` JWT claim. |
| `sentinel-edge` | 8788 (localhost) | On-prem appliance — the ONLY camera-touching component. ONVIF discovery, owns a go2rtc sidecar (:1984), serves snapshots, joins the tailnet. Config-light (no DB). |
| `sentinel-app` | — | The SvelteKit web UI. Calls control (`API_BASE`) for everything; gateway (`GATEWAY_BASE`) for live. |
| `sentinel-postgres` | 5432 | Shared Postgres (+ pgvector for face embeddings). |
| `sentinel-ai` | — | Face detect/embed sidecar (InsightFace, GPU/CPU; RunPod). `AI_SERVICE_URL`. |

`crates/app-core` is the shared domain library (devices, workflows, alerts, ONVIF, go2rtc,
agent/vision, `ports`, `db`) linked by control, flow, and edge. The frontend `lib/data.ts`
is the single API layer (`api.*` typed client).

## Data flow

**Edge ⇄ control (over the public internet — the edge dials OUT, no inbound):**
- `GET /config/cameras?wait=25` — long-poll for the tenant's camera list; control wakes it
  via `AppState::bump` (a `Notify`) the instant the web edits a camera. Health headers
  (`X-Edge-Tailscale`, `X-Edge-Ffmpeg`, `X-Box-Host`, `X-Edge-Version`) ride this request →
  `store::touch_box_by_token` stamps `tenant_boxes`.
- `POST /config/discovered` — ONVIF sweep (every 5 min).
- `POST /config/device-moved` — cameras whose IP changed (reconciled by stable ONVIF id).
- `POST /config/streaming` — IPs the edge has a resolved stream for (drives the Devices dot).
- `POST /config/logs` — recent log lines (edge + go2rtc child).
- `GET /config/commands` — pending `rescan` (tenant) / `restart` (box) commands, consumed.

**Browser ⇄ control** (`API_BASE`, JWT bearer): everything except live video.

**Browser ⇄ gateway ⇄ edge go2rtc** (live): the browser opens `wss://gateway/api/ws?src=cam_<ip>&token=<jwt>`;
the gateway resolves the tenant's box host via control `/internal/box-upstream` (cached 30s),
then bridges to `wss://<box-host>/api/ws` over the tailnet. Snapshots/frames go the same way.

**flow → edge go2rtc** (`GetCurrentFrame`, snapshots in graphs): flow resolves
`edge_base(tenant)` = the tenant's box host and fetches `/api/frame.jpeg?src=cam_<ip>`.

## Multi-tenancy

`tenant_id` is on every domain table (migration 0011). Control derives the tenant from the
user's JWT; the edge derives it from its `BOX_TOKEN` (sha256 → `tenant_boxes`); flow is
inherently multi-tenant (loads all tenants' graphs, keys everything by `(tenant, …)`). The
SSE relays filter strictly by the caller's tenant.

## The tailnet (how the cloud reaches LAN cameras)

go2rtc binds **localhost only** (`127.0.0.1:1984`) on the edge — never on the LAN/internet.
The edge joins a **Tailscale** tailnet and runs `tailscale serve --bg 1984`, exposing go2rtc
at `https://<magicdns-host>/` (TLS via MagicDNS). The VPS (gateway + flow) are also tailnet
nodes, so they reach the edge privately. The box reports its MagicDNS host on the heartbeat
(`X-Box-Host` → `tenant_boxes.magicdns_host`); `box_host_for_tenant` returns the
most-recently-seen one. **If the edge isn't connected to the tailnet, Live + GetCurrentFrame
fail** while localhost go2rtc still works — see [edge ref](edge.md).

## Key persisted state (Postgres, `crates/app-core/migrations`)

- `devices` (PK `(tenant_id, ip)`, carries stable ONVIF `id`), `configured_cameras`,
  `streaming_cameras`, `discovered_cameras`.
- `tenant_boxes` (box registry: token hash, magicdns_host, version, last_seen + health:
  `tailscale_state`, `ffmpeg_ok`, `health_at`).
- `workflows` (graph JSONB), `workflow_runs` + `workflow_run_logs` (run history),
  `workflow_cron_state`, `tenant_events` + `tenant_event_categories` (event registry),
  `box_logs` (edge log tail).
- `tenants`, users, roles, permissions (RBAC); `alerts`; `notification_targets`; `map_rooms`.
