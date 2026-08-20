# macOS installer — a signed `.pkg`

**Board card:** #82 macOS installer — signed + notarized `.pkg` (In Progress)
**Goal:** A customer downloads one file, double-clicks it, types a pairing code, and has a working
appliance. No terminal, no Homebrew, no `curl | sudo sh`.

**Depends on [[box-pairing-code]] (#81)** — without it the installer has no way to receive the box's
identity. Sibling: [[edge-windows-installer]] (#83).

---

## STATUS (2026-07-14): code done, blocked on Apple signing certs

Everything except the signature is **built, committed, and validated end-to-end against production**:

  universal (x86_64 + arm64), plus the launchd daemon. Scripts under `deploy/macos-pkg/`.
  codesigned (malformed Mach-O → unnotarizable), and `--disable-gpl` gives LGPL certainty +
  `--disable-autodetect` keeps it free of the build host's Homebrew dylibs. See Open decisions #3.
  ones were killed by Gatekeeper), the unenrolled edge serves the pairing page, a code redeems into
  a real box token + edge auth key written to `edge.env`, and the re-launched edge pulls the
  tenant's cameras and reports online.

**Last mile — signing. The signing path is wired** (`task edge-pkg-signed` expects `MACOS_APP_CERT`,
`MACOS_PKG_CERT`, `NOTARY_PROFILE`); unsigned, Gatekeeper refuses the package (worse than the curl|sh
it replaces), so this is the last step, not polish.

**This does NOT require the organization account.** A paid **individual** Apple Developer account
issues both Developer ID certs (Application + Installer) and notarizes the same way — the only
difference is cosmetic: the installer's signer name shows the person's legal name instead of the
company. So the fast path is:
- **Now:** sign with the existing individual account to ship #82. No code/script rework — signing is
  just three env vars.
- **In parallel:** org conversion for JDV GLOBAL SOLUTIONS LTDA (**D-U-N-S 574899435**), emailed to
  Apple Support (topic "Inscrição no programa") on **2026-07-14**. When it lands, regenerate the two
  certs in the company name and re-sign — a five-minute swap.

**To sign (either account):** create Developer ID **Application** + **Installer** certs in the portal
(CSR from Keychain Access), run `xcrun notarytool store-credentials <profile> --apple-id … --team-id
… --password <app-specific>`, hand the two exact cert identity strings to `task edge-pkg-signed`, and
verify with `spctl -a -vvv -t install` → must say "accepted, source=Notarized Developer ID".

**Two follow-ups surfaced during the test (not blockers):**
1. **launchd bootstrap fails on the test Mac (error 5), and it's host policy, not the package** — a
   SentinelOne-managed corporate machine blocking a new system LaunchDaemon. The `postinstall` was
   already hardened to never fail the install because of it (the daemon comes up on next boot). On an
   unmanaged Mac, launchd `KeepAlive` handles the restart-after-enroll the journey relies on.
2. **Enrollment UX gap** — the Boxes install view only advances when the box checks in, with a mute
   window between "code accepted" and "box online" (and no feedback at all if the box is slow or
   never boots). Worth an intermediate "Code accepted — waiting for the appliance to start…" state,
   fired at redeem time. Candidate for its own card.

---

## `.pkg`, not `.dmg`

A `.dmg` is a disk image you drag an **app bundle** out of, into `/Applications`. The edge is a
**headless launchd daemon** — there is nothing to drag and nothing to open. The right artifact is a
`.pkg`: it can lay files down as root, write `/etc/sentinel-edge/edge.env`, register the launchd
daemon, and run a postinstall script. (A `.dmg` *containing* a `.pkg` is a thing people do for
presentation; it just adds a step. Skip it.)

## What it fixes beyond the double-click

mode on a clean Mac — a customer appliance should not require a package manager for developers.

fetched, nothing is compiled, and it works on a machine with no developer tooling and no network
beyond reaching control. This is only possible now: since [[edge-pure-rust-motion]] (#79) the edge
links nothing native, so there are no OpenCV dylibs to ship or match.

## Phase 1 — build the package (unsigned)

All CLI, no Xcode project:

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
   (hardened runtime is required for notarization).
3. **Developer ID Installer** cert → sign the package:
   `productbuild --sign "Developer ID Installer: …"`.
4. **Notarize**: `xcrun notarytool submit … --wait`, then `xcrun stapler staple` so it validates
   offline.

All four steps run on a Mac, from the CLI — this is scriptable as `task edge-pkg` and does not need
a CI runner (GitHub Actions is billing-blocked anyway — see [deploy ref](../ref/deploy.md)).

## Verification

- On a **clean** Mac (a fresh VM, no Homebrew, no Xcode): download → double-click → installs →
  browser opens the enrollment page → paste code → the box appears in the Boxes page with private link
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
   impossible, not merely unattractive:
     failed strict validation"* (malformed Mach-O). Notarization requires **every** executable
     unnotarizable installer, which means Gatekeeper blocks the download. Dead end.
   - evermeet.cx's builds are well-formed but **Intel-only**, and download at a crawl from a
     personal server — not a dependency a release build should have.
   - Building with `--disable-gpl --disable-nonfree` also settles the licence: **LGPL v2.1+**,
     verified, on a binary we redistribute commercially — instead of inheriting whatever a third
4. ~~**Universal binary (arm64 + x86_64) or two packages?**~~ **Universal**, and it was nearly free:
   since #79 the edge links nothing native, so both Mac architectures build from one machine with no
   download, no choice for the customer to get wrong.

## Out of scope (v1)

- A GUI/tray app. The edge is a service; the web app is the interface.
- MDM/enterprise deployment profiles.
- Sparkle-style in-app update UI.
