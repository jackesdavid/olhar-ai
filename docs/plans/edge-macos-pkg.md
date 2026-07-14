# macOS installer — a signed `.pkg`

**Board card:** #82 macOS installer — signed + notarized `.pkg` (ToDo)
**Goal:** A customer downloads one file, double-clicks it, types a pairing code, and has a working
appliance. No terminal, no Homebrew, no `curl | sudo sh`.

**Depends on [[box-pairing-code]] (#81)** — without it the installer has no way to receive the box's
identity. Sibling: [[edge-windows-installer]] (#83).

---

## `.pkg`, not `.dmg`

A `.dmg` is a disk image you drag an **app bundle** out of, into `/Applications`. The edge is a
**headless launchd daemon** — there is nothing to drag and nothing to open. The right artifact is a
`.pkg`: it can lay files down as root, write `/etc/sentinel-edge/edge.env`, register the launchd
daemon, and run a postinstall script. (A `.dmg` *containing* a `.pkg` is a thing people do for
presentation; it just adds a step. Skip it.)

## What it fixes beyond the double-click

`install.sh` currently provisions ffmpeg through **Homebrew**, and gives up with *"Install ffmpeg
first: brew install ffmpeg"* if it isn't there (`sentinel-edge/install.sh`). That is a real failure
mode on a clean Mac — a customer appliance should not require a package manager for developers.

The `.pkg` **bundles the payload**: the edge binary, `go2rtc`, and a static `ffmpeg`. Nothing is
fetched, nothing is compiled, and it works on a machine with no developer tooling and no network
beyond reaching control. This is only possible now: since [[edge-pure-rust-motion]] (#79) the edge
links nothing native, so there are no OpenCV dylibs to ship or match.

## Phase 1 — build the package (unsigned)

All CLI, no Xcode project:

- Payload root: `/opt/sentinel-edge/{sentinel-edge,go2rtc,ffmpeg}` + `/Library/LaunchDaemons/io.sentinel.edge.plist`.
- `pkgbuild --root <payload> --scripts <scripts> --identifier io.sentinel.edge --version <edge version>`
- `productbuild --package … --distribution …` for the outer product archive.
- **postinstall**: `launchctl bootout` the old instance (the `Load failed: 5` lesson already learned
  in `install.sh`), `bootstrap` the new one, then `open http://localhost:8788` — the edge's
  enrollment page from #81. The user is *at* the machine; localhost is exactly right.
- Keep `install.sh` working. It stays the headless/Linux path.

Ship the version in the filename (`Sentinel-Edge-0.5.38.pkg`) and serve it from the same dist
server the binaries come from (`/ibbf/edge-dist/`, see [deploy ref](../ref/deploy.md)).

## Phase 2 — sign + notarize (the part that costs money)

Unsigned, Gatekeeper refuses to open it and the customer sees *"cannot be opened because it is from
an unidentified developer."* **That is worse than the `curl | sh` we're replacing** — so this phase
is not optional; it's the point.

Needs, in order:

1. **Apple Developer Program** — US$99/year. Enroll as the company (an org enrollment needs a D-U-N-S
   number and takes days-to-weeks; a sole-proprietor enrollment is faster). *This is the long pole —
   start it before the code.*
2. **Developer ID Application** cert → sign the binaries inside the payload:
   `codesign --force --options runtime --timestamp -s "Developer ID Application: …" sentinel-edge go2rtc ffmpeg`
   (hardened runtime is required for notarization).
3. **Developer ID Installer** cert → sign the package:
   `productbuild --sign "Developer ID Installer: …"`.
4. **Notarize**: `xcrun notarytool submit … --wait`, then `xcrun stapler staple` so it validates
   offline.

All four steps run on a Mac, from the CLI — this is scriptable as `task edge-pkg` and does not need
a CI runner (GitHub Actions is billing-blocked anyway — see [deploy ref](../ref/deploy.md)).

## Verification

- On a **clean** Mac (a fresh VM, no Homebrew, no Xcode): download → double-click → installs →
  browser opens the enrollment page → paste code → the box appears in the Boxes page with tailnet
  `serving` and `ffmpeg ✓`.
- Reboot the Mac → the daemon comes back by itself.
- `spctl -a -vvv -t install Sentinel-Edge-*.pkg` → "accepted, source=Notarized Developer ID".
- Install over an existing install (the upgrade path) → no `Load failed: 5`, config preserved.
- Auto-update still works (`maybe_self_update` → it fetches the *binary*, not the pkg — confirm the
  two paths don't fight; see Open decisions).

## Open decisions (grill before building)

1. **Who owns the Apple Developer account** — a company entity (needs D-U-N-S) or an individual? An
   individual account shows the person's name to the customer at install time. For a product sold to
   businesses, that reads wrong. **This is a business decision and it gates the schedule.**
2. **Does the `.pkg` replace `maybe_self_update`, or coexist?** The edge currently self-updates by
   downloading a bare binary and swapping it (`main.rs:37`). A signed-package world normally updates
   through a signed package. Simplest coexistence: keep binary self-update (the swapped binary is
   still individually signed, so the hardened runtime is satisfied), and use the `.pkg` for
   first install only. Confirm the swapped binary keeps its signature.
3. ~~**Bundle ffmpeg — which build, and its licence?**~~ **Resolved by trying it.** We build ffmpeg
   ourselves (`deploy/macos-pkg/build-ffmpeg.sh`), because the prebuilt option turned out to be
   impossible, not merely unattractive:
   - **`codesign` rejects 3 of the 4 macOS binaries `ffmpeg-static` publishes** — *"main executable
     failed strict validation"* (malformed Mach-O). Notarization requires **every** executable
     inside the package to carry a Developer ID signature, so an unsignable ffmpeg means an
     unnotarizable installer, which means Gatekeeper blocks the download. Dead end.
   - evermeet.cx's builds are well-formed but **Intel-only**, and download at a crawl from a
     personal server — not a dependency a release build should have.
   - Building with `--disable-gpl --disable-nonfree` also settles the licence: **LGPL v2.1+**,
     verified, on a binary we redistribute commercially — instead of inheriting whatever a third
     party's CI configured (most prebuilt macOS ffmpegs are GPL). It ships as a separate executable
     that go2rtc *spawns*; we never link it.
   - `ffprobe` is bundled too — the edge reads a finished clip's duration with it.
4. ~~**Universal binary (arm64 + x86_64) or two packages?**~~ **Universal**, and it was nearly free:
   since #79 the edge links nothing native, so both Mac architectures build from one machine with no
   cross-toolchain. go2rtc ships both; ffmpeg we build for both; `lipo` fuses each. One file, one
   download, no choice for the customer to get wrong.

## Out of scope (v1)

- A GUI/tray app. The edge is a service; the web app is the interface.
- MDM/enterprise deployment profiles.
- Sparkle-style in-app update UI.
