# macOS installer (`.pkg`) — card #82

One signed, notarized file. The customer double-clicks it, types a pairing code, and has a working
appliance. No terminal, no Homebrew, no `curl … | sudo sh`.

```sh
task edge-pkg            # unsigned — for testing on THIS Mac only
task edge-pkg-signed     # signed + notarized — the release artifact
```

Output: `deploy/macos-pkg/build/Sentinel-Edge-<version>.pkg`.

## Why a `.pkg` and not a `.dmg`

A `.dmg` is a disk image you drag an **app bundle** out of and into `/Applications`. The edge is a
**headless launchd daemon** — there is nothing to drag and nothing to open. A `.pkg` can lay files
down as root, register the daemon, and run a postinstall script, which is exactly what's needed.

## What's in it

| Path | What |
|---|---|
| `/opt/sentinel-edge/sentinel-edge` | the edge, **universal** (x86_64 + arm64) |
| `/opt/sentinel-edge/go2rtc` | the streaming sidecar, universal |
| `/opt/sentinel-edge/ffmpeg`, `ffprobe` | **bundled**, universal, LGPL — see below |
| `/opt/sentinel-edge/LICENSE.ffmpeg` | ffmpeg's licence, as redistribution requires |
| `/etc/sentinel-edge/edge.env` | `CONTROL_URL` only — **no box token** |
| `/Library/LaunchDaemons/io.sentinel.edge.plist` | the daemon |

**One package for every customer.** It carries no identity: the appliance gets its box token from a
**pairing code** typed into the page the postinstall opens (`http://localhost:8788`, card #81). That
is what makes a *signed* package possible at all — a per-box installer would have to be re-signed on
demand, with Apple's private key sitting on a server.

Universal binaries are a dividend of card #79: with OpenCV gone, the edge links nothing native, so
both Mac architectures build from one machine with no cross-toolchain.

## Why we build ffmpeg ourselves

ffmpeg is **not optional and not just for snapshots** — go2rtc shells out to it to turn the camera's
H264 into the JPEG frames the motion and tracking pipeline reads. No ffmpeg, no detection.

We compile it (`build-ffmpeg.sh`) instead of bundling a prebuilt static binary, for two reasons, the
first of which we found by trying:

1. **The popular prebuilt macOS binaries cannot be signed.** `codesign` rejects 3 of the 4 that
   `ffmpeg-static` publishes — *"main executable failed strict validation"* (malformed Mach-O).
   Notarization requires **every** executable inside the package to carry a Developer ID signature,
   so an unsignable ffmpeg means an unnotarizable installer, which means Gatekeeper blocks the
   download. (evermeet.cx's builds are well-formed but Intel-only, and download at a crawl from a
   personal server — not something a release should depend on.)
2. **Licence certainty.** We redistribute this binary commercially. `--disable-gpl --disable-nonfree`
   guarantees **LGPL v2.1+**, rather than inheriting whatever a third party's CI configured — most
   prebuilt macOS ffmpegs are GPL. It ships as a separate executable that go2rtc *spawns*; we never
   link it.

The build keeps ffmpeg's default codec set (no `--disable-everything`). A hand-picked minimal build
would be much smaller, but go2rtc composes its own command lines, and a missing demuxer would
surface as a broken camera at a customer's site rather than here.

## Signing — the part that costs money

**Unsigned, Gatekeeper refuses to open the package** ("cannot be opened because it is from an
unidentified developer"). That is a *worse* experience than the `curl | sh` this replaces, so
signing is not a polish step — it's the point of the card.

`task edge-pkg-signed` expects:

| Env | What |
|---|---|
| `MACOS_APP_CERT` | `Developer ID Application: <Org> (TEAMID)` — signs the four executables |
| `MACOS_PKG_CERT` | `Developer ID Installer: <Org> (TEAMID)` — signs the package |
| `NOTARY_PROFILE` | a profile name from `xcrun notarytool store-credentials` |

Both certificates come from the **Apple Developer Program** (US$99/year). Enrolling as an
organisation needs a D-U-N-S number and takes days to weeks — **start that before the code is
ready**, it's the long pole.

Verify a release with:

```sh
spctl -a -vvv -t install build/Sentinel-Edge-*.pkg   # → "accepted … source=Notarized Developer ID"
```

## Testing an unsigned build

Gatekeeper will block it on any Mac. Right-click → **Open** bypasses it for a machine you control.
