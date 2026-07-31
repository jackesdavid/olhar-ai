# Windows installer — a signed `.exe`

**Board card:** #83 Windows installer — signed `.exe` (ToDo)
**Goal:** The same double-click install on Windows: one signed `.exe`, a pairing code, a running
service. Turns "buy an appliance" into "download this onto the PC you already own."

**Depends on [[edge-windows]] (#80)** — there is no Windows binary yet — **and on
[[box-pairing-code]] (#81)** for the enrollment. Sibling: [[edge-macos-pkg]] (#82).

---

## Order matters

This card is third, and trying to pull it forward will just stall:

1. **#79 (done)** — the edge links no native library, so a Windows build is a plain
   `cargo build --target x86_64-pc-windows-msvc` instead of a vcpkg/OpenCV-DLL project.
2. **#80** — the binary itself: paths (`%ProgramData%`), the Windows service, the self-update
   children.
3. **#81** — pairing, so the installer has something to ask for.
4. **This card** — the packaging around all of it.

## Phase 1 — build the installer (unsigned)

**Inno Setup** over WiX/MSI: it is far simpler for what we need (a service + three files + a
firewall rule), and it produces exactly the single `.exe` a customer expects. MSI buys us
enterprise/GPO deployment we have no customer for yet.

  macOS `.pkg`: no customer should need a package manager).
- Register the Windows **service** with failure-actions restart (`sc failure … actions= restart/5000`)
  — the Boxes-page Restart button works by the edge calling `exit(0)` and trusting the supervisor to
  bring it back, so that contract must hold here or Restart silently kills boxes.
  confirm on a clean machine rather than adding a rule by reflex).
- Finish by launching `http://localhost:8788` — the edge's enrollment page (#81).

## Phase 2 — sign (the part that costs money)

Unsigned, **SmartScreen** shows "Windows protected your PC" and hides the Run button behind *More
info*. For a security product being sold to a business, that is a conversion killer.

- **OV certificate** (~US$100–400/yr): the warning goes away only after the binary accumulates
  reputation — which a low-volume product may never do.
- **EV certificate** (~US$300–500/yr, hardware token or cloud HSM): SmartScreen reputation is
  **immediate**. For our volume, this is almost certainly the right buy.
- Signing can be done off-Windows with `osslsigncode`, so the release path doesn't require a Windows
  CI runner — but building the `.exe` with Inno Setup does (Wine works; a Windows VM is honest).

## Verification

- Clean Windows 11 VM, no dev tools: download → run → install → browser opens the enrollment page →
  paste the code → the box appears in Boxes with the right platform badge, tailnet `serving`,
- Reboot → the service comes back on its own. Boxes-page Restart → it comes back.
- ONVIF discovery finds a LAN camera; Live plays; motion + tracking events reach the cloud.
- SmartScreen shows no warning on a machine that has never seen the binary.
- Uninstall leaves no service behind and doesn't take `/etc`-equivalent config with it if the user
  is reinstalling.

## Open decisions (grill before building)

1. **EV or OV certificate?** EV costs more and needs a token/HSM, but it's the only one that removes
   the SmartScreen warning on day one. **A business decision — and the same conversation as the
   Apple Developer account in #82.**
2. **Do we sell Windows as an appliance, or only as a "try it on your own PC" tier?** The answer
   changes how much support surface we take on (antivirus false positives, corporate policy,
   sleeping laptops that stop recording). It should be decided before we ship it, not after the
   first support ticket.
3. **Inno Setup needs Windows (or Wine) to build.** With GitHub Actions billing-blocked, that means
   a local Windows VM in the release path. Acceptable, or worth reconsidering WiX (which builds from
   .NET on macOS/Linux)?
4. **A sleeping PC records nothing.** If the appliance is the shop's front-desk machine, the
   installer should probably disable sleep — that's a real behaviour change to the customer's
   computer and needs consent, not a silent registry write.

## Out of scope (v1)

- ARM Windows.
- MSI / GPO / domain deployment.
- A tray icon or any GUI. The web app is the interface.
