# Face Pipeline on Primary (HD) Stream — Implementation Plan

**Board card:** Face pipeline on primary (HD) stream (ToDo)
**Goal:** Capture faces from the camera's **main/HD** stream instead of the low-res substream, so
stored faces are 3–6× larger and sharper — with detection and the stored crop coming from the **same
HD frame** (no temporal mismatch, no bbox scaling). Selectable **per tenant**, and made affordable on
the edge by **GPU-accelerated H.264 decode** — gated on an **edge capability probe** so it's never
enabled on hardware that can't actually do it.

## Root cause (researched 2026-07-20)
The entire face path runs on the **substream**. The edge's per-camera frame publisher
(`sentinel-edge/src/frames.rs:207`) grabs frames via `app_core::go2rtc::frame_from(ip)`, hardwired to
`frame.jpeg?src=cam_<ip>` — and `cam_<ip>` is the **preview/substream** (`go2rtc.rs`: *"Low-res"*).
Face detection reuses that motion frame (`frames.rs:252 → face::detect_and_push`), so both the bbox
and the stored JPEG are substream-res (~640px wide → ~20–60px faces; Anne's crop was 22px). The HD
stream **already exists** in go2rtc as `cam_<ip>_hd` (`stream_name_hd`, served behind the Live HD
toggle) — it's just never used for capture. There is **no stream selector** on the Camera node or
anywhere in capture.

**Decision (locked):** run the whole face pipeline on the HD frame — detect + crop on one HD frame.
Rejected the "HD-upgrade-on-hit" approach because detecting on sub@T1 and grabbing HD@T2 risks the
face having moved between the two frames.

## Phase 1 — Edge: an HD frame source
- Add `app_core::go2rtc::frame_from_hd(agent, base, ip)` — same as `frame_from` but `src=stream_name_hd(ip)`
  (`cam_<ip>_hd`). Keep `frame_from` for the preview consumers.
- Warmth: an on-demand grab of a cold HD stream is slow/stale, so the HD stream must be **warm** for
  cameras doing face capture. Options: (a) the frame publisher decodes HD directly (always warm, see
  Phase 2), or (b) extend the existing `cameras_always_warm` behaviour to keep `cam_<ip>_hd` warm.
  Prefer (a) when the tenant selects HD — one decode, always warm, zero mismatch.

## Phase 2 — Edge: face detection on the HD frame
- When the tenant's face-capture stream = HD, the per-camera source thread (`frames.rs`) publishes the
  **HD** frame (decode `cam_<ip>_hd`). Motion + `face::detect_and_push` then run on that same HD frame —
  detection bbox and the emitted `jpeg` are the same HD pixels. No scaling, no second grab.
- Consequence: motion + tracking + snapshots for that camera also run on HD (they consume the shared
  published frame). That's the CPU cost of HD (see Open decisions). The browser Live/preview still uses
  the separate `cam_<ip>` sub go2rtc stream — unaffected.
- `face_detected` event, bbox semantics, and the cloud pipeline ([[face-pipeline-architecture]]) are
  unchanged — only the pixels get bigger.

## Phase 3 — Per-tenant selector (settings)
- New tenant setting, e.g. `face_capture_stream` ∈ `{ "preview", "hd" }` (default `preview` — no
  behaviour change on upgrade). Lives with the other tenant settings (same table/pattern as
  `cameras_always_warm`), behind `settings.read`/`settings.write`.
- **UI:** a selector on the Settings page — "Face capture quality: Preview (low CPU) / HD (best photos,
  more edge CPU)".
- **Delivery to edge:** ship the value in the camera-config payload the edge pulls from control (same
  channel that already carries `cameras_always_warm`). The edge picks `frame_from`/HD publish per this
  flag, re-reading on config refresh (no restart).
- Future extension (out of scope v1): per-**camera** override, not just per-tenant.

## Status (2026-07-20)
- **MEASUREMENT VERDICT — GPU/Phase 5 is DEAD.** Split metric on the real box (HD 1280×720):
  **grab ~405–595 ms, decode ~5 ms.** The bottleneck is go2rtc's per-request `frame.jpeg` cold
  production (starving the hub to ~2 fps vs the 4 fps target), NOT decode CPU. VideoToolbox has
  nothing to save. → **Phase 5 (GPU-in-go2rtc) abandoned.**
- **FIX = warm persistent decoder (edge 0.5.49, `4309287`).** `capture.rs`: a supervised, always-on
  ffmpeg per camera reads go2rtc's warm RTSP re-serve → continuous MJPEG; the hub reads the freshest
  frame (grab → ~0 ms) and falls back to `frame.jpeg` only when no fresh frame exists. Resilient:
  watchdog kills a wedged ffmpeg, exits caught, supervisor respawns with backoff. **Verify on-box:**
  `capture[]` should show grab≈0 ms + ~4 fps; `capture-decoder` restart lines prove resilience.
- (below: earlier phase status)

### Prior status (2026-07-20)
- **Phases 1–3 + measurement DONE + deployed** (`ec4ec3e`, edge **0.5.47** `2743cc2`): `frame_from_hd`
  (grab `cam_<ip>_hd`); the edge publisher reads `face_capture_stream` on the box `recording_config`
  and grabs HD vs preview (same frame → motion + face); a **Settings HD-face-capture toggle** gated on
  the box's reported `hwaccel`; and a per-frame **capture-cost log** (`capture[ip]: mode=… WxH avg_decode=…ms fps`)
  every ~10s → the Logs page. Edge 0.5.46→0.5.47 published. Box VideoToolbox capability verified.
- **KNOWN GAP — measurement is HD-on-CPU only.** go2rtc still decodes HD on the CPU (`#media=video`, no
  hwaccel), so the log shows preview-CPU vs HD-**CPU** — NOT HD+GPU. The harness is correct (it times the
  real grab+decode), so once Phase 5 wires GPU into go2rtc the SAME log reflects the GPU cost.
- **Rollout (in-product):** Boxes→Update (0.5.47) → Settings→HD on → Boxes→Restart (config reload) →
  Logs for the numbers, Faces for bigger crops. (Setting is boot/reload-time, like `cameras_always_warm`.)
- **Next: Phase 5** — a dedicated `cam_<ip>_face` go2rtc stream with `-hwaccel videotoolbox`, SEPARATE from
  `cam_<ip>_hd` (leave Live HD untouched), pointed at by `frame_from_hd`; needs on-box verification that
  VideoToolbox actually engages. Then the measurement covers all 3 modes (substream / HD-CPU / HD-GPU).

### (earlier) Phase 6 status
- **Phase 6 cloud side DONE + deployed** (`df8e801`): edge `hwaccel.rs` probe (functional `-init_hw_device`
  test, bounded subprocess), `X-Edge-Hwaccel` header, control ingest (migration **0064** `tenant_boxes.hwaccel`,
  `box_health_headers` + both `touch_box` upserts, COALESCE/backward-compatible), and a "GPU decode · <method>"
  badge on the Boxes page. All compiles; cloud deployed.
- **NOT released to boxes yet.** The probe only reports once a box runs the new edge. Edge version stays
  **0.5.45** on purpose — the 0.5.46 bump must be **atomic with `edge-publish`** (build+upload the binary,
  THEN bump `LATEST_EDGE_VERSION`). A bare source bump made control advertise 0.5.46 with no binary →
  self-update loop; reverted (VPS env + Cargo.toml back to 0.5.45). See [[edge-publish-version-check]].
- **Next on #120:** `edge-publish` for **both** macOS + Linux assets, verify the probe picks the right method
  on a real GPU box (badge lights up), then build Phases 1–5 (HD publish + per-tenant setting + GPU config).

## Hardware-acceleration reference (the probe's candidate methods)
For the edge the workload is **decode** (H.264/H.265 → frames). Candidates, best-first per platform:

| API | Vendor / OS | Decode | go2rtc | ffmpeg build needs |
|---|---|---|---|---|
| **VAAPI** | Intel + **AMD** iGPU/dGPU, Linux | ✅ | `#hardware=vaapi` | `--enable-vaapi` + `mesa-va-drivers` |
| **QSV** | Intel only, Linux | ✅ | `#hardware=qsv` | `--enable-libvpl`/libmfx |
| **CUDA / NVDEC** | NVIDIA only | ✅ | `#hardware=cuda` | `--enable-cuda --enable-cuvid` |
| **VideoToolbox** | macOS (Intel + Apple Silicon) | ✅ | custom `ffmpeg:` template | ships in macOS ffmpeg |
| **V4L2 M2M** | ARM SoCs (RPi, Rockchip) | ✅ | `#hardware` | distro ffmpeg |
| **Vulkan Video** | cross-vendor, Linux (newer) | ✅ | custom | `--enable-vulkan` |
| ~~VDPAU / AMF~~ | legacy / Windows-encode | — | — | skip for the edge |

Platform → method the probe should try:
- **Linux + AMD** → `vaapi` (Mesa); `vulkan` as a modern fallback. (No CUDA/QSV on AMD.)
- **Linux + Intel** → `qsv` first, then `vaapi`.
- **Linux + NVIDIA** → `cuda`.
- **macOS (incl. Intel Macs, e.g. [[mac-edge-test-box]])** → `videotoolbox` (NOT vaapi/qsv/cuda — those follow the OS, not the chip).
- **ARM SoC** → `v4l2m2m`.

## Phase 5 — GPU-accelerated decode (companion; makes HD affordable on the edge)
HD's real cost is decoding 1080p/4K H.264 for `frame.jpeg` (go2rtc runs **ffmpeg** to transcode
H.264→JPEG — the frame path today is CPU). Offloading that decode to the GPU's fixed-function video
engine (NVDEC/QuickSync/VAAPI/VideoToolbox) is **10–50× more efficient than CPU** — a genuinely strong
win, unlike GPU for *inference* ([[face-compute-stays-cpu]]: only 2.4×, overhead-bound). Don't conflate
the two: video **decode** on a dedicated ASIC ≠ shader-core inference.

- go2rtc supports HW accel via its `ffmpeg:` source form (`#hardware=vaapi|cuda|qsv|videotoolbox|…`).
  `config_yaml` (`crates/app-core/src/go2rtc.rs`) would emit the hardware source form when the edge
  reports a working method (see Phase 6). Requires an **hwaccel-enabled ffmpeg** in the edge runtime
  image ([[edge-linux-fedora-build]]; Mac box has VideoToolbox — [[mac-edge-test-box]]).
- Relevant mainly WITH HD; for the 640×360 substream CPU decode is already cheap.

## Phase 6 — Edge capability probe (REQUIRED — never enable GPU blind)
The edge must **prove** GPU decode works before we offer/enable it — listing a hwaccel is not proof
(drivers/permissions/headless can still fail). So:
- On boot (+ periodically / on demand), the edge probes:
  1. `ffmpeg -hwaccels` → the *advertised* methods.
  2. **Functional test** per candidate: actually decode a tiny sample with `-hwaccel <m>` (e.g. decode a
     few frames of a known clip or a live keyframe to null) and check the exit code — the real gate.
  3. Pick the best working method (`vaapi`/`qsv`/`cuda`/`videotoolbox`) or `none`.
- **Report it to control**: extend the edge health/registration payload (today it already sends
  `X-Edge-Ffmpeg`; add `gpu_decode: { available, method }`). Control persists it per box.
- **Gating rule (hard):** HD face capture cannot be enabled unless EITHER (a) the box reports a working
  hwaccel method, OR (b) the user explicitly opts into **CPU-HD** (shown as heavier, with the measured
  cost). The default HD selector shows "HD (GPU · <method>)" when available, greyed with "needs GPU /
  CPU-only" otherwise. Never silently enable HD on a box that can't sustain it.
- **Runtime fallback:** if a configured hwaccel fails to start at runtime, the edge reverts to CPU
  decode and re-reports `method: none` so the UI reflects reality (and can warn).
- Surface the detected capability on the edge/box detail — e.g. "GPU decode: QuickSync ✓" / "VAAPI ✓" /
  "none — CPU only". The probe candidates come from the reference matrix above (per-platform order).

## Phase 7 — Ship
- Bump edge `Cargo.toml` + `LATEST_EDGE_VERSION`; build + `task edge-publish` (native OpenCV — build from
  `sentinel-edge/`). Deploy control + app for the setting + UI. See [[edge-publish-version-check]].

## Open decisions (grill before building)
1. **Whole publisher → HD, or a dedicated HD face-feed?** Publishing HD moves motion+tracking+snapshots
   to HD too (simplest, one decode, always warm — but heavier steady CPU). A dedicated second HD decode
   only for the face path keeps motion cheap but doubles decode. Recommendation: **publisher → HD when
   tenant selects HD** (accept the motion-on-HD cost; motion downscales internally anyway).
2. **Edge CPU headroom / GPU.** Measure real sub vs main resolutions + edge core count before enabling —
   HD decode + YuNet on HD is the cost. GPU decode (Phase 5) removes most of it *if* the box can do it
   (Phase 6 probe). Order of work: the probe (Phase 6) likely comes FIRST — it tells us whether HD is
   even viable on a given box and whether CPU-only HD is acceptable there. Consider a per-camera cap /
   HD frame-rate throttle regardless.
3. **Setting granularity:** per-tenant now; per-camera later. Confirm per-tenant is enough for v1.
4. **Snapshots on HD:** switching the publisher to HD also makes workflow Camera Snapshots HD for that
   tenant — desirable, but confirm it's acceptable (bigger snapshot payloads).

## Out of scope (v1)
- Per-camera stream override.
- Auto-selecting HD only when a face is present (adaptive) — start with a static tenant flag.
- Super-resolution / upscaling of existing substream faces (not worth it; recapture on HD instead).
