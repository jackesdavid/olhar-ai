# Full edge on the device (iOS + Android) — Implementation Plan

**Board card:** #144 full edge on device — phone runs sentinel-edge (1–2 cameras)
**Goal:** run the **real `sentinel-edge`** on the phone — RTSP ingest of 1–2 LAN cameras, GStreamer
decode, motion + detection, recording, box registration — so a phone is a first-class edge. iOS is
accepted as **short-lived** (foreground / limited background); it is the proving ground for the
Android edge and the way to keep **one edge codebase across all targets** (parity).

## Why this, and how it differs from #143

- **#143 (light edge)** = the phone's *own* camera → face events into the cloud pipeline. Tiny, no
  GStreamer, foreground-only. Shipped Phase 1–2.
- **#144 (this, full edge)** = the phone runs the *whole edge* against *other* (LAN RTSP) cameras —
  the same job an appliance does. This is the #138 GStreamer work's north star: the linked
  `gstreamer-rs` pipeline was chosen precisely because it ports to a phone (no `gst-launch`
  subprocess). The Mac Intel edge is done and shipped; this extends that same code to arm64 iOS/Android.

## The parity play — ONE edge, compiled per platform

`sentinel-edge` is a standalone Rust crate (Axum HTTP server, `retina` RTSP, `tract` detection, MOG2
motion, recording, box registration). The plan is **not** to rewrite it in JS/Swift/Kotlin — it is to
**compile the existing crate as a static library** for `aarch64-apple-ios` / `aarch64-linux-android`,
embed it in the app via a thin native module (start/stop), and bundle GStreamer per platform. Same
code, same behaviour, everywhere — which is exactly the operator's parity requirement.

## Feasibility findings (2026-07-25)

- **iOS rustup target**: was absent; added `aarch64-apple-ios`.
- **Pure-Rust cross-compile: ✅ (2026-07-25).** `sentinel-edge` builds for `aarch64-apple-ios`
  **without** `gst-linked` in 3m04s — one benign `unused var` warning, nothing else. The whole dep
  graph (`retina` RTSP, `tract` detection, `axum` HTTP, `image`, `sysinfo`, MOG2) cross-compiles to
  iOS arm64. This is the big de-risk: the Rust side of a phone edge is real; **GStreamer linking is
  the only remaining unknown.**
- **GStreamer iOS link: ✅ (2026-07-25).** Installed the **GStreamer iOS SDK 1.28.5** (`.pkg`, 484 MB,
  matches the Mac's 1.28.5) → `~/Library/Developer/GStreamer/iPhone.sdk/GStreamer.framework` (universal:
  arm64 = platform 2 = **iOS device**, x86_64 = simulator). The edge **cross-compiles AND links with
  `gst-linked` for `aarch64-apple-ios`** — a 113 MB arm64 iOS binary, GStreamer statically pulled in.
  **The whole media stack builds for the phone.** How it was made to link (reusable):
  1. The iOS SDK is a **monolithic framework, no `.pc` files** (built for Xcode, not pkg-config).
     Synthesised 8 `.pc` files (`glib/gobject/gio/gmodule/gstreamer/-base/-app`) with
     `Cflags: -I<framework>/Headers`, pointed at via `PKG_CONFIG_PATH` + `PKG_CONFIG_ALLOW_CROSS=1`.
  2. **`system-deps` does not forward `-framework` from a `.pc`** — the compile found headers but the
     final link had undefined `_gst_*` symbols. Forcing the framework link
     (`RUSTFLAGS="-L framework=<iPhone.sdk> -l framework=GStreamer"`, i.e. a
     `cargo:rustc-link-lib=framework=GStreamer` in a build script) resolved every symbol.
  The Android twin is the **GStreamer Android SDK** (`.aar`/NDK) — same pattern expected.
- **Edge deps**: `retina` (RTSP, pure Rust), `tract` (ML, pure Rust), `opencv` (test-only, optional),
  `gstreamer` (optional, the linked path). No OpenSSL (rustls). Encouraging for cross-compile.

## Phase 1 — build + link spike (the gate)

The `stage4`-equivalent: prove the edge **compiles and links** for the phone before any app work.

1. **Pure-Rust cross-compile** to `aarch64-apple-ios` (no gst) — **✅ done, 3m04s, clean.** The dep
   graph compiles for iOS arm64.
2. **GStreamer iOS link — ✅ done** (see feasibility above). `sentinel-edge` + `gst-linked` links for
   `aarch64-apple-ios`. The synthetic `.pc` + `-l framework=GStreamer` recipe is captured there; the
   proper home for the link flag is a small `build.rs` in `sentinel-edge` gated on the iOS target.
3. **`crate-type = ["staticlib"]` + C ABI — ✅ done (2026-07-25).** The edge is now a lib + a thin
   bin: `main.rs` is a 4-line delegator, everything moved to `lib.rs` (the crate root), and
   `Cargo.toml` declares `crate-type = ["staticlib", "rlib"]`. Added `#[no_mangle] start_edge()`
   (boots `run()` on a background thread, idempotent) and `stop_edge()`. Verified: the Mac bin still
   builds; `libsentinel_edge.a` builds for `aarch64-apple-ios` with `gst-linked`, and `nm` shows
   `_start_edge` / `_stop_edge` as exported text in the arm64 slice. The `.a` is 939 MB in **debug**
   (static GStreamer, unstripped) — a release+strip build is the shippable size.
4. Repeat for `aarch64-linux-android` + the GStreamer Android SDK.

**Gate: PASSED, and the edge RUNS on iOS.** `libsentinel_edge.a` (arm64 iOS, GStreamer
linked) exports `start_edge`/`stop_edge`. What remains is app plumbing — the native module that links
the `.a` and bundles the GStreamer framework — and repeating the link for Android.

## Phase 2 — native module: start/stop the edge from the app — LINK ✅ (2026-07-25)

- **A local Expo module** (`sentinel-mobile/modules/sentinel-edge`) links `libsentinel_edge.a` and
  calls the C ABI via Swift `@_silgen_name` (no header/modulemap). `build-ios-lib.sh` builds the
  release lib (122 MB, no-gst first) into the pod; an **Edge tab** boots it and polls
  `localhost:8788/health`.
- **The app builds and links on iOS.** `npx expo prebuild` + `xcodebuild -destination
  'generic/platform=iOS'` produced `OlharAI.app` (Release-iphoneos) with **`_start_edge` present in
  the final binary** — the edge is embedded, no undefined symbols. The podspec's
  `Security/SystemConfiguration/CoreFoundation` + `c++/resolv` covered every std/dep symbol; dead-strip
  took the 122 MB static lib down to a 45 MB app binary.
- **RUNTIME PROVEN ON DEVICE (2026-07-25).** Installed on an iPhone 11, tapped Start → the Edge tab
  shows **"Edge is up — /health answered ✓"** with `{"enrolled":false,"service":"sentinel-edge",
  "status":"ok"}`. The real edge (tokio + axum + retina) **boots and serves HTTP inside iOS.** Two
  fixes got it there: the unenrolled branch now serves `/health` (a phone boots without a box token),
  and the listen address defaults to `127.0.0.1` on iOS (the app is on-device; iOS wants localhost).
- **Next:** GStreamer (`GST=1 build-ios-lib.sh`) so it can decode a camera — the known iOS wrinkle is
  **static plugin registration** (the monolithic framework links plugins statically; `gst_init` won't
  auto-discover them, they need explicit registration). Then Phase 3 (paths/box identity on a phone)
  and Android.

## Phase 3 — the edge's platform assumptions

The edge assumes an appliance; on a phone several must be revisited (name real code):
- **Paths** (`sentinel-edge/src/paths.rs`) — the phone's app sandbox, not `/opt/sentinel-edge`.
- **go2rtc/ffmpeg** — already gone on the GStreamer path (#138), so nothing to bundle there. ✅
- **Tailscale** (`tailscale.rs`) — a phone edge on the same LAN as its cameras may not need the
  tailnet at all (cameras are directly reachable); the cloud reaches it only for config, which it can
  poll outbound. Decide: no-tailnet phone edge vs. Tailscale iOS/Android.
- **Cameras** — the phone is **on the Wi-Fi LAN**, so `rtspsrc` reaches cameras directly, same as an
  appliance. This is what makes 1–2 cameras realistic.

## Phase 4 — register as a real box + report

- Reuse the box identity: the phone pairs/self-registers (extend #143's `POST /boxes/self-register`
  to a full box, or the appliance pairing) and reports health/cameras like any edge, so it shows in
  the Boxes UI and workflows bind to its cameras.

## Open decisions

1. **Full crate vs. slimmer phone edge.** Parity says compile the whole crate. If iOS linking or
   binary size forces it, a phone-specific subset (ingest + detect + push, no local recording/HTTP)
   is the fallback — decided by Phase 1.
2. **iOS background.** Accept foreground/short-lived for the test; do not engineer around the OS. The
   real 24/7 target is Android (foreground service).
3. **GStreamer SDK size in the app.** The iOS/Android GStreamer SDK is large; bundling only the ~13
   plugins we use (as the Mac `.pkg` does) keeps the app reasonable.

## Out of scope (v1)

- 24/7 iOS operation (OS-blocked; that's Android's job).
- More than ~2 cameras on a phone (thermal/CPU; the point is to prove parity, not density).
- Windows/Coral edges (separate columns on the #138 matrix).
