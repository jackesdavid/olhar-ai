# sentinel-edge on Linux (Fedora / Bazzite)

The edge links **OpenCV dynamically** (the MOG2 motion watcher), so the run host's OpenCV
major must match the build's. Real appliances are **Bazzite** — immutable Fedora (rpm-ostree)
— where you can't `dnf install` OpenCV on the host. So on Linux we build on Fedora and, on an
immutable host, run the edge as a **self-contained container** (matching OpenCV baked in).

## Build

```sh
# Cross-build the binary from macOS in a Fedora container → sentinel-edge/dist/…
task edge-linux-build              # Fedora 42 / OpenCV 4.11 (set FEDORA=41 to match an older Bazzite)
task edge-linux-publish            # + scp to the VPS dist server + redeploy control

# Or build the self-contained runtime image (OpenCV + ffmpeg + go2rtc + tailscale baked in):
docker build --platform linux/amd64 -f deploy/edge-linux/Dockerfile --target runtime \
  -t ghcr.io/jackesdavid/sentinel-edge:latest .
```

Fedora splits OpenCV per-module: the `opencv` metapackage ships core/imgproc/imgcodecs/videoio
but **not** `opencv-video` (MOG2 lives there) — the Dockerfile + `install.sh` install it
explicitly, else the edge dies with `libopencv_video.so.NNN: cannot open shared object file`.

## Run on a MUTABLE Linux box (Debian/Ubuntu/regular Fedora)

The normal installer works — it installs OpenCV + ffmpeg + tailscale and a systemd unit:

```sh
curl -fsSL https://control.thesentinels.online/edge/install.sh | sudo sh -s -- \
  --control https://control.thesentinels.online --box-token sbx_… --authkey tskey-… --hostname my-box
```

## Run on Bazzite / Silverblue (IMMUTABLE — recommended: container)

The host can't `dnf install` OpenCV, so run the self-contained image. It runs `tailscaled`
inside, so it needs host networking + the tun device + NET_ADMIN. The `--env-file` is the same
`edge.env` the installer writes (CONTROL_URL, BOX_TOKEN, TS_AUTHKEY, TS_HOSTNAME, …):

```sh
# /etc/sentinel-edge/edge.env  (from the Boxes page one-liner values):
#   CONTROL_URL=https://control.thesentinels.online
#   BOX_TOKEN=sbx_xxxxxxxx
#   TS_AUTHKEY=tskey-auth-xxxx
#   TS_HOSTNAME=my-box

podman run -d --name sentinel-edge --restart=always \
  --network=host --cap-add=NET_ADMIN --cap-add=NET_RAW --device=/dev/net/tun \
  -v sentinel-edge-tailscale:/var/lib/tailscale \
  -v sentinel-edge-data:/opt/sentinel-edge \
  --env-file /etc/sentinel-edge/edge.env \
  ghcr.io/jackesdavid/sentinel-edge:latest
```

`--network=host` is required: ONVIF WS-Discovery uses LAN UDP multicast, and go2rtc binds
localhost. For boot persistence, generate a quadlet/systemd unit: `podman generate systemd
--new --name sentinel-edge` (or a `~/.config/containers/systemd/sentinel-edge.container`).

## Alternative: distrobox (Fedora userspace on the immutable host)

```sh
distrobox create --name sentinel-edge --image fedora:42 \
  --additional-flags "--cap-add=NET_ADMIN --device=/dev/net/tun"
distrobox enter sentinel-edge -- sudo dnf install -y opencv opencv-video ffmpeg-free tailscale
# then run the binary + go2rtc inside (tailscaled must run in the box or on the host).
```
The container image is simpler and self-contained; prefer it unless you need an interactive box.
