# sentinel-edge

The **on-prem edge gateway** — the only component that talks to local cameras. Phase 2
of the cloud decomposition (full architecture: `docs/plans/multi-tenant-gateway-edge.md`).

Each client site runs one edge box. It:
- **owns go2rtc** — ingests the site's RTSP cameras and serves MSE + JPEG frames,
  bound to **`127.0.0.1:1984`** (localhost only — never the LAN or public internet);
- serves a small **local REST API** (snapshots for `sentinel-flow`'s `FrameSource`,
  the device + stream list);
- **joins the Tailscale tailnet** and `tailscale serve`s go2rtc, so the central
  `sentinel-gateway` reaches it privately.

It links `app-core` (device repo, go2rtc config, ONVIF + snapshot capture) and reads
the shared on-prem Postgres for the device list.

## Run

```sh
task db-up
task edge        # GO2RTC_BIN defaults to the desktop app's bundled binary
# or:
DATABASE_URL=postgres://sentinel:sentinel@localhost/sentinel \
EDGE_BIND=127.0.0.1:8788 GO2RTC_BIN=../app/src-tauri/binaries/go2rtc \
cargo run -p sentinel-edge
```

| Env | Default | Purpose |
|---|---|---|
| `DATABASE_URL` | `postgres://sentinel:sentinel@localhost/sentinel` | device list |
| `EDGE_BIND` | `0.0.0.0:8788` | local REST listen address |
| `EDGE_OWN_GO2RTC` | `1` (on) | spawn/own go2rtc; `0` to reuse an existing :1984 |
| `GO2RTC_BIN` / `GO2RTC_CONFIG` | `go2rtc` / `go2rtc.yaml` | binary + generated config |
| `TS_AUTHKEY` / `TS_HOSTNAME` / `TS_TAGS` | — | Tailscale enrollment (opt-in) |

## API

| Method | Path | Purpose |
|---|---|---|
| `GET` | `/health` | liveness + device count + `owns_go2rtc` |
| `GET` | `/devices` | the cameras (`ip`, `name`, `has_stream`) |
| `GET` | `/streams` | playable go2rtc streams (`cam_<ip>` + player URLs) |
| `GET` | `/snapshot/:ip` | **binary** JPEG (go2rtc fast-path → ONVIF/ffmpeg fallback) |
| `POST` | `/reload` | re-read devices from Postgres (+ restart go2rtc if owned) |

## Tailscale (private link to the gateway)

With `TS_AUTHKEY` set the edge runs, at startup:

```sh
tailscale up --authkey=<key> --hostname=$TS_HOSTNAME --advertise-tags=$TS_TAGS
tailscale serve --bg 1984
```

go2rtc is then reachable **only** over the tailnet at
`https://<TS_HOSTNAME>.<tailnet>.ts.net` (MagicDNS TLS). Without `TS_AUTHKEY` it skips
Tailscale (LAN/one-box dev). Needs `tailscaled` installed + MagicDNS & HTTPS certs
enabled on the tailnet.

## Not yet (later phases)

Dynamic per-tenant enrollment broker (gateway mints short-lived auth keys),
offboarding via the Tailscale API, edge-local vision co-location. See the plan.
