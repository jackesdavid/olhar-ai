# sentinel-edge on Linux (Fedora / Bazzite)

The edge is **pure Rust** — it links no native library at all (card #79; motion's MOG2 lives in
`sentinel-edge/src/mog2.rs`). The only thing still tying a build to its host is **glibc**: a binary
built against Fedora's glibc won't start on an older Debian/Ubuntu. So we ship two Linux assets, and
`install.sh` fetches the matching one by package manager.

> **This used to be much worse.** The edge linked OpenCV dynamically, so the binary was coupled to
> the run host's OpenCV *soname* — a Bazzite on a different Fedora major needed its own build, the
> installer had to layer OpenCV via `rpm-ostree` and **reboot** before the box could even start, and
> the appliance ran as a self-contained container purely to guarantee a matching soname. All of that
> is gone. A static **musl** build would collapse the last of it (the two assets → one, runs on any
> Linux); that's the natural next step.

## Build

| Asset | Built on | Coupled to | Host distros |
|---|---|---|---|
| `sentinel-edge-linux-x86_64`     | Fedora 42 (`Dockerfile`)           | glibc (Fedora 42) | Fedora / Bazzite (dnf, rpm-ostree) |
| `sentinel-edge-linux-x86_64-gnu` | Ubuntu 24.04 (`Dockerfile.ubuntu`) | glibc (noble)     | Debian 12 / Ubuntu 24.04 (apt) |

```sh
task edge-linux-build              # Fedora 42 → sentinel-edge-linux-x86_64
task edge-linux-build-ubuntu       # Ubuntu 24.04 → sentinel-edge-linux-x86_64-gnu
task edge-linux-publish            # build (Fedora) + scp to the VPS dist server + redeploy control

docker build --platform linux/amd64 -f deploy/edge-linux/Dockerfile --target runtime \
  -t registry.gitlab.com/olharai-grupo/olharai/sentinel-edge:latest .
```

## Run on a MUTABLE Linux box (Debian/Ubuntu/regular Fedora)


```sh
curl -fsSL https://control.thesentinels.online/edge/install.sh | sudo sh -s -- \
  --control https://control.thesentinels.online --box-token sbx_… --authkey tskey-… --hostname my-box
```

## Run on Bazzite / Silverblue (IMMUTABLE)

`install.sh` now works here too — the edge links nothing, so it starts immediately instead of
snapshots are affected, not live video/motion/tracking/recording).

tailscale, so nothing touches the host). It runs `tailscaled` inside, so it needs host networking +
the tun device + NET_ADMIN. The `--env-file` is the same `edge.env` the installer writes
(CONTROL_URL, BOX_TOKEN, TS_AUTHKEY, TS_HOSTNAME, …):

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
  registry.gitlab.com/olharai-grupo/olharai/sentinel-edge:latest
```

localhost. For boot persistence, generate a quadlet/systemd unit: `podman generate systemd
--new --name sentinel-edge` (or a `~/.config/containers/systemd/sentinel-edge.container`).

## Alternative: distrobox (Fedora userspace on the immutable host)

```sh
distrobox create --name sentinel-edge --image fedora:42 \
  --additional-flags "--cap-add=NET_ADMIN --device=/dev/net/tun"
```
The container image is simpler and self-contained; prefer it unless you need an interactive box.
