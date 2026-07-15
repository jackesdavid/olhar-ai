# Per-platform installer download button

**Board card:** #84 Per-platform installer download (Review)

## STATUS (2026-07-15): shipped + deployed

`GET /edge/installers` is live and returns all three (verified against prod, every URL 200):
`macos pkg 82 MB` · `windows zip 130 MB` · `linux sh (one-liner)`. The install view leads with a
per-OS Download button (browser-detected) + an "other platforms" disclosure, then the pairing code
as step 2. Control + app deployed (commit d75866c).

**Caught by the exists-check while shipping:** the signed macOS `.pkg` from #82 had been built +
validated locally but never published to the dist server — so it was correctly *omitted* from the
manifest (no 404 button) until I `scp`'d it to `/ibbf/edge-dist/`. The metadata gate did exactly its
job. Lesson folded into memory [[apple-signing-82]]: publishing the `.pkg` is a real release step,
not just building it.

---
**Goal:** A customer sees one obvious button that hands them the right installer for the machine
they're on — the last mile of "turn a PC you already own into an appliance." No `curl` string to
copy, no guessing which file.

Closes the loop on the installers we just built: [[edge-macos-pkg]] (#82, signed `.pkg`),
[[edge-windows-installer]] (#83, Windows `.zip`), and the pairing flow [[box-pairing-code]] (#81).

---

## What exists to build on

- Control already serves the dist server at **`/edge/<file>`** (`sentinel-control/src/http.rs:50`,
  `nest_service("/edge", ServeDir::new(EDGE_DIST_DIR))`), plus `/install.sh` and `/edge-latest`
  (which returns `{version}` from `LATEST_EDGE_VERSION`).
- The artifacts are **already published** there:
  - macOS: `Sentinel-Edge-<ver>.pkg` (signed, notarized).
  - Windows: `SentinelEdge-Windows-<ver>.zip`.
  - Linux: the `curl … | sudo sh` one-liner (`/install.sh`), no single file.
- The Boxes install view (`sentinel-app/src/routes/settings/boxes/install/+page.svelte`) is where a
  user lands right after **Add box** — the natural home for "now install it on the appliance."

## The design decision that matters

**Don't hardcode filenames in the web app.** The version is in `LATEST_EDGE_VERSION` on control; the
app shouldn't guess `Sentinel-Edge-0.5.39.pkg`. Add one control endpoint that names the current
installers, and the app just renders what it's told. That keeps a version bump a control-only change.

## Phase 1 — control: an installers manifest

`GET /edge/installers` (public, like `/edge-latest`) → the per-platform download set for the current
version:

```json
{
  "version": "0.5.39",
  "installers": [
    { "os": "macos",   "label": "macOS (Apple Silicon + Intel)", "kind": "pkg", "url": "/edge/Sentinel-Edge-0.5.39.pkg" },
    { "os": "windows", "label": "Windows 10/11 (64-bit)",        "kind": "zip", "url": "/edge/SentinelEdge-Windows-0.5.39.zip" },
    { "os": "linux",   "label": "Linux (one-line install)",      "kind": "sh",  "url": "/install.sh" }
  ]
}
```

- Build the filenames from `LATEST_EDGE_VERSION` + the fixed naming scheme. **Only list a file that
  actually exists** on disk in `EDGE_DIST_DIR` (a `std::fs::metadata` check) — a button that 404s is
  worse than an absent one, and this also covers "Windows not published yet for this version."
- Include `size_bytes` per file (from the same `metadata`) so the button can show "130 MB".

## Phase 2 — web: the download UI

- `sentinel-app/src/lib/api.ts`: `edge.installers()` → the manifest.
- Detect the user's OS from `navigator.userAgent` (macOS / Windows / Linux). A small helper, not a
  library.
- In the install view, above/beside the pairing code: a primary **"Download for <this OS>"** button
  (the detected platform), plus a small "other platforms" disclosure for the rest. If detection is
  ambiguous, show all three equally.
- macOS/Windows are a direct file download (`<a href download>`). Linux is the copyable one-liner we
  already render — keep it, just fold it under the Linux option.
- House style: no nested ternaries in markup — a named helper picks the primary installer from the
  manifest + detected OS.

## Phase 3 — where else the button belongs

- The Boxes page **empty state** (no boxes yet) should also lead with "Add your first box" → the same
  install view. Don't scatter download buttons; route everyone through the install view so the code +
  download sit together.

## Verification

- On a Mac browser: the install view leads with the macOS `.pkg` button; it downloads and Gatekeeper
  accepts it (already validated in #82). Windows/Linux available under "other platforms".
- On a Windows browser: leads with the Windows `.zip`.
- Bump `LATEST_EDGE_VERSION` (or publish a new file) → the button follows with no web change.
- Delete the Windows file from dist → its button disappears (the `metadata` gate), macOS still shows.

## Open decisions (grill before building)

1. **Does download gate on anything?** A box row already exists (created by **Add box**) before the
   download — so the installer is generic and the *code* is the per-box secret. No auth needed on the
   file itself (it carries nothing secret). Confirm we're OK serving the installers to any
   authenticated tenant (they are — the .pkg/.zip are identical for everyone).
2. **`.zip` vs a real `.exe` for Windows** — today it's a `.zip` the user unzips + runs `install.ps1`.
   The button will say "Download (.zip)" until the Inno Setup `.exe` wrapper lands (#83). Fine as an
   honest interim; the label should say `.zip` so there's no surprise.
3. **Show the version + size on the button?** Leaning yes — "macOS · 82 MB · v0.5.39" sets
   expectations for a big download.

## Out of scope (v1)

- A public marketing download page (this is the in-app, post-Add-box flow).
- Auto-detecting arch beyond OS (the macOS `.pkg` is universal; Windows is x64-only for now).
- The Inno Setup `.exe` (that's #83).
