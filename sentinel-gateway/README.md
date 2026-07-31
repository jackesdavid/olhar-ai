# sentinel-gateway

The **public streaming bridge**. Phase 2 of the cloud decomposition (full
architecture: `docs/plans/multi-tenant-gateway-edge.md`).

End-user browsers connect here; the gateway proxies **live video (MSE over
WebSocket)** and **JPEG snapshots** through to a client's on-prem `sentinel-edge`
`tailscale serve`'d, so it's never exposed directly.

```
```

It's a thin **scoped reverse proxy** — no `app-core`, no DB, no domain types:
- `GET /api/ws` → **WebSocket bridge** to `{upstream}/api/ws` (MSE fMP4 both ways).
- everything else GET → **streamed passthrough** to the upstream (`/api/frame.jpeg`,
- `GET /player?src=cam_<ip>` → redirects to the proxied player forced to MSE.

**Single-tenant for now:** one configured upstream (`EDGE_UPSTREAM`). The per-tenant
registry lookup + JWT slot into `AppState::resolve_upstream` later — no transport change.

## Run

```sh
task gateway
# or:
EDGE_UPSTREAM=http://127.0.0.1:1984 GATEWAY_BIND=127.0.0.1:8789 \
cargo run -p sentinel-gateway
```

| Env | Default | Purpose |
|---|---|---|
| `GATEWAY_BIND` | `0.0.0.0:8789` | listen address |
| `TS_AUTHKEY` / `TS_HOSTNAME` / `TS_TAGS` | — | join the tailnet at startup (opt-in) |

## Verify

```sh
curl localhost:8789/health
curl "localhost:8789/api/frame.jpeg?src=cam_<ip>" -o f.jpg     # snapshot passthrough
# open in a browser → camera plays via MSE through the gateway:
#   http://localhost:8789/player?src=cam_<ip>
```

The upstream scheme drives the transport: `http://` → `ws://`, `https://` → `wss://`,
so dev (loopback) and prod (tailnet TLS) use the same code.

## Not yet (later phases)

JWT `Claims` extractor (tenant_id); `tenant_boxes` registry + `resolve_upstream` DB
lookup (offline → 503); enrollment broker; gateway multiplexing; restricting the
passthrough surface for public exposure. See the plan.
