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
- **GStreamer for iOS**: **not installed** — there is no `GStreamer.framework`, only the Homebrew Mac
  build. iOS needs the official **GStreamer iOS SDK** (~1 GB from gstreamer.freedesktop.org, a
  `.framework` with the same plugins). Its Android twin is the **GStreamer Android SDK** (`.aar`/NDK).
  These are the load-bearing external dependency and the big download — pull deliberately.
- **Edge deps**: `retina` (RTSP, pure Rust), `tract` (ML, pure Rust), `opencv` (test-only, optional),
  `gstreamer` (optional, the linked path). No OpenSSL (rustls). Encouraging for cross-compile.

## Phase 1 — build + link spike (the gate)

The `stage4`-equivalent: prove the edge **compiles and links** for the phone before any app work.

1. **Pure-Rust cross-compile** to `aarch64-apple-ios` (no gst) — **✅ done, 3m04s, clean.** The dep
   graph compiles for iOS arm64.
2. **GStreamer iOS SDK**: install the `.framework`, point `pkg-config` / the `gstreamer-rs` build at
   it, and cross-compile `sentinel-edge` **with `gst-linked`** for iOS. The known risk mirrors the
   Mac `.pkg`: GStreamer's own dylibs/plugins must be bundled and their load paths rewritten for the
   app sandbox.
3. **`crate-type = ["staticlib"]`** (a lib target beside the bin) so the app can link it; a C ABI
   `start_edge()/stop_edge()` entry point.
4. Repeat for `aarch64-linux-android` + the GStreamer Android SDK.

**Gate:** if the edge links with GStreamer on iOS arm64, the rest is integration; if it cannot, that
finding reshapes the whole card (fall back to a slimmer phone-specific ingest).

## Phase 2 — native module: start/stop the edge from the app

- A React Native native module (Swift/Kotlin) that loads the static lib and calls `start_edge(config)`
  on a background thread, `stop_edge()` on screen close / background. The edge's Axum server binds
  localhost on the phone; the app talks to it exactly as the web app talks to an appliance edge.
- Foreground service (Android) / the honest short-lived window (iOS) — capture must not pretend to
  outlive what the OS allows.

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
