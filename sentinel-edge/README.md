# sentinel-edge

The **on-prem edge gateway** — the only component that talks to local cameras. Phase 2
of the cloud decomposition (full architecture: `docs/plans/multi-tenant-gateway-edge.md`).

Each client site runs one edge box. It:
  bound to **`127.0.0.1:1984`** (localhost only — never the LAN or public internet);
- serves a small **local REST API** (snapshots for `sentinel-flow`'s `FrameSource`,
  the device + stream list);
  `sentinel-gateway` reaches it privately.

the shared on-prem Postgres for the device list.

## Install (client appliance — one command)

The client runs this once on their box; it installs the edge, fetches the

```sh
curl -fsSL https://github.com/olharai/Sentinel/releases/latest/download/install.sh \
  | sudo sh -s -- \
      --authkey tskey-auth-xxxx \
      --hostname tenant-001 \
      --tags tag:tenant-001 \
      --control https://control.example.com
```

Binaries + `install.sh` are published per platform by `.github/workflows/release.yml`
on a version tag (`git tag v0.1.0 && git push --tags`): Linux x86_64/aarch64 (static
musl) and macOS arm64/x86_64.

## Run (from source, dev)

```sh
task db-up
# or:
DATABASE_URL=postgres://sentinel:sentinel@localhost/sentinel \
cargo run -p sentinel-edge
```

| Env | Default | Purpose |
|---|---|---|
| `DATABASE_URL` | `postgres://sentinel:sentinel@localhost/sentinel` | device list |
| `EDGE_BIND` | `0.0.0.0:8788` | local REST listen address |

## API

| Method | Path | Purpose |
|---|---|---|
| `GET` | `/devices` | the cameras (`ip`, `name`, `has_stream`) |

## Reaching the edge

Nothing dials into the box. The edge is outbound-only (card #144): it reaches control over
HTTPS, holds one outbound SSE command stream, and pushes live video out to the gateway.

## Motion detection (OpenCV MOG2)

The edge runs one continuous **OpenCV MOG2** background-subtraction watcher per camera
(`src/motion.rs`), reads each stream locally, and POSTs a normalized motion event to control
(`/config/motion`, box-token). The edge is a pure sensor — per-graph sensitivity is applied
in the cloud. Tunables (env): `MOTION_FPS` (6), `MOTION_FLOOR_PCT` (0.5),
`MOTION_MIN_INTERVAL_SECS` (10), `MOTION_MIN_AREA` (300), `MOTION_PROC_W`/`MOTION_PROC_H`
(480×270), `MOTION_MOG2_HISTORY` (500), `MOTION_MOG2_VAR` (16).

**Build needs OpenCV** (native). Runtime needs the OpenCV dylibs (standard paths); the
build additionally needs **libclang** (bindgen):

- Linux: `apt install libopencv-dev libclang-dev` (or distro equivalents) — `install.sh`
  installs the runtime OpenCV automatically.
- macOS: `brew install opencv pkg-config`. Building also needs libclang on dyld's path —
  cargo's clang-sys loads it at build-script runtime, so pass it explicitly:
  ```sh
  LIBCLANG_PATH=/Library/Developer/CommandLineTools/usr/lib \
  DYLD_FALLBACK_LIBRARY_PATH=/Library/Developer/CommandLineTools/usr/lib \
  cargo build --release -p sentinel-edge
  ```
  (Runtime needs no env — the binary links the brew OpenCV dylibs via their install path.)

## Not yet (later phases)

Edge-local vision co-location. See the plan.
