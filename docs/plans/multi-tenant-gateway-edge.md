# Multi-Tenant Camera Streaming Platform — Architecture Plan

> Source: provided by the user (designed in a prior cloud session). This is the
> authoritative plan for the **sentinel-gateway** (cloud bridge) and the
> **sentinel-edge** (on-prem, the only component that talks to local cameras).

## Overview

Multi-tenant surveillance/streaming. Each client runs **one** on-prem edge box (go2rtc
ingesting RTSP cameras). A central **Axum gateway** bridges public end-users to the
private edge boxes over a **Tailscale** tailnet. A web app lets each client’s users
view only their own streams.

## Topology

```
client browser/app  ──public HTTPS/WSS──▶  Axum gateway  ──tailnet──▶  client's go2rtc edge box
```

- **Private tailnet:** only the central gateway + client edge boxes. One device (tagged resource) per client.
- **Public app layer:** end-user browsers/apps ↔ gateway over HTTPS/WSS, authenticated by JWT.
- End-user devices **never** join the VPN. The gateway is the only bridge between the two layers.

---

## Decisions

### Streaming format
- **MSE** (WebSocket carrying fMP4), ~1s latency. **Not** WebRTC.
- Reason: proxies cleanly through the gateway as WS-over-TCP, passes through Cloudflare, no STUN/TURN needed.
- WebRTC reserved for later only if sub-second latency justifies running TURN.

### Fan-out
- **Per-viewer passthrough** (stateless proxy); go2rtc handles source fan-out on the box.
- Ship this first. Instrument concurrent-viewers-per-camera and per-site upstream bandwidth.
- Add gateway multiplexing **only** if a camera draws many simultaneous viewers **or** client uplink is constrained (multiplexing requires MSE init-segment caching — avoid until measured need).

### Connectivity — Tailscale
- Tailnet members: central gateway + edge boxes only.
- **Persistent tagged nodes** (not ephemeral — boxes are 24/7).
- **Per-tenant tag** `tag:tenant-<id>` for identification + defense-in-depth. Isolation is trivial: edges only ever talk to the gateway, never each other.
- **Enrollment via broker pattern:**
  1. OAuth client on the tailnet (scope `auth_keys` + tags). Secret stays on the central backend only.
  2. Edge authenticates to the central backend → backend mints a short-lived auth key → returns it.
  3. Edge runs `tailscale up` with that key.
- **Offboarding:** backend deletes the node via the Tailscale API (frees the hostname, cleans the tagged-resource count).

### Addressing — MagicDNS
- Set hostname **at enrollment** → deterministic name, known before the box connects (no lookup).
- Store `tenant-<id>.<tailnet>.ts.net` in `tenant_boxes.magicdns_host`.
- Rules: DNS-safe slug (lowercase, alphanumeric + hyphens, no underscores, ≤63 chars); unique per box; **delete-then-replace** on swap to keep the name stable.
- Tailnet DNS settings: enable **MagicDNS** + **HTTPS certificates** (the `serve` cert is issued for the MagicDNS name).

### Edge — go2rtc exposure
```yaml
# go2rtc.yaml
api:
  listen: "127.0.0.1:1984"   # localhost only — never LAN/public
```
```bash
# enroll + expose over tailnet (TLS via MagicDNS)
tailscale up --auth-key=<brokered> --advertise-tags=tag:tenant-123 --hostname=tenant-123
tailscale serve --bg 1984
```
go2rtc is reachable **only** through the tailnet at `https://tenant-123.<tailnet>.ts.net`.

### Gateway — Axum
Registry: Postgres `tenant_boxes(tenant_id, magicdns_host, online, tag, ...)`; `online` driven by edge heartbeat.

```rust
async fn resolve_upstream(db: &PgPool, tenant: Uuid) -> Result<String, AppError> {
    let row = sqlx::query!(
        "SELECT magicdns_host, online FROM tenant_boxes WHERE tenant_id = $1", tenant
    ).fetch_one(db).await?;
    if !row.online { return Err(AppError::BoxOffline); } // → 503
    Ok(row.magicdns_host)
}

// MSE (WebSocket)
async fn stream_ws(
    ws: WebSocketUpgrade, State(st): State<AppState>,
    claims: Claims, Path(camera): Path<String>,
) -> Result<Response, AppError> {
    let host = resolve_upstream(&st.db, claims.tenant_id).await?;
    let upstream = format!("wss://{host}/api/ws?src={camera}");
    Ok(ws.on_upgrade(move |client| bridge_ws(client, upstream))) // pipe both directions
}

// HLS / snapshots (HTTP) — stream through, no buffering
async fn stream_http(
    State(st): State<AppState>, claims: Claims,
    Path((camera, rest)): Path<(String, String)>,
) -> Result<Response, AppError> {
    let host = resolve_upstream(&st.db, claims.tenant_id).await?;
    let up = st.http.get(format!("https://{host}/api/{rest}?src={camera}")).send().await?;
    let ct = up.headers().get("content-type").cloned();
    let mut res = Body::from_stream(up.bytes_stream()).into_response();
    if let Some(ct) = ct { res.headers_mut().insert("content-type", ct); }
    Ok(res)
}
```
- `bridge_ws`: split both sockets, pipe both ways, converting between axum and tungstenite `Message` types.
- Public TLS: Caddy / Cloudflare / rustls in front (WSS-over-TCP passes Cloudflare fine).

### Auth & isolation
- JWT carries `tenant_id`; gateway extractor validates it.
- Enforced at the gateway: the upstream address comes **only** from the validated JWT → registry lookup, never from the client. A tenant cannot address another tenant’s box.

### Failure handling
- `online` flag → clean 503 instead of hanging on a dead tailnet dial.
- Connect timeouts on both the reqwest client and the WS connect to prevent stuck-connection pileup.

### Cost — Tailscale
- Billed per user-seat; **user devices free/unlimited**; **tagged resources free up to 50/month**, overage beyond.
- Each client = 1 tagged resource → **~$0 marginal cost up to ~50 clients**; seats are just your own team.
- **~50 clients** = the crossover to evaluate self-hosting **Headscale** (free control plane, you operate it). Below that, managed Tailscale.

---

## How this fits the existing decomposition

- **sentinel-edge** (this repo, on-prem box): owns go2rtc (RTSP cameras, `127.0.0.1:1984`),
  enrolls into the tailnet via the broker, runs `tailscale serve 1984`, heartbeats the
  gateway. Also keeps the **local** snapshot REST (`GET /snapshot/<ip>`) that
  `sentinel-flow`'s `FrameSource` uses on the LAN. It is a full on-prem Sentinel install
  (local Postgres: devices/persons/workflows + sentinel-flow + this edge gateway).
- **sentinel-gateway** (new, cloud): multi-tenant Axum bridge with its OWN central
  Postgres (`tenants`, `tenant_boxes`, auth). JWT in → `resolve_upstream` → proxy
  MSE-WS + HTTP(snapshots/HLS) to the tenant's box over the tailnet. Hosts the
  enrollment broker (mint short-lived Tailscale auth keys) + heartbeat receiver.
- Two databases by design: edge-local (device/flow state) vs gateway-central
  (tenant/box/auth registry).

## Build order (proposed)

1. **Scaffold both Axum services** compiling + runnable locally, integration points
   (Tailscale enrollment, JWT verification) behind config/seams with dev defaults
   (edge↔gateway over a configured URL; a dev JWT verifier). Core proxy logic real:
   `tenant_boxes` registry, `resolve_upstream`, `stream_http` passthrough, `bridge_ws`,
   heartbeat.
2. **Real JWT** verification (issuer/JWKS or shared secret) + the `Claims` extractor.
3. **Real Tailscale**: broker endpoint (OAuth client → short-lived auth key), MagicDNS
   hostname assignment, edge `tailscale up` + `serve`, offboarding via the API.
4. **Frontend**: MSE player (WS) pointed at the gateway; per-tenant stream list.
5. Instrument viewers/bandwidth; revisit multiplexing + Headscale at ~50 clients.
