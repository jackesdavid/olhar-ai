# Edge without OpenCV — pure-Rust motion

**Board card:** #79 Edge without OpenCV — pure-Rust motion (ToDo)
**Goal:** Delete the edge's only native dependency. OpenCV is what couples the binary to the host
(soname on Linux, DLLs on Windows), forces the Bazzite appliance to ship as a self-contained
container, and is the single thing standing between us and a `cargo build --target
x86_64-pc-windows-msvc` that just works.

Blocks: [[edge-windows]] (#80). Builds on: [[edge-frame-hub]] (#77).

---

## Why now

Before #77, OpenCV was smeared across the motion watcher. After it, the **entire** OpenCV surface of
the edge is two files (`sentinel-edge/src/frames.rs`, `sentinel-edge/src/motion.rs`) and exactly ten
calls:

| OpenCV call | Replacement | Where it lives today |
|---|---|---|
| `imgcodecs::imdecode` | `image::load_from_memory` | already used by the detector |
| `imgproc::resize` | `image::imageops::resize` (Triangle) | already used by the detector |
| `imgproc::morphology_ex(OPEN)` + `get_structuring_element` + `morphology_default_border_value` | `imageproc::morphology::open_mut` | **`imageproc` is already an app-core dep** |
| `core::count_non_zero` | count the mask's non-zero bytes | trivial |
| `imgproc::find_contours` + `contour_area` + `bounding_rect` | `imageproc::region_labelling::connected_components` → per-label pixel count + bbox in one pass | `imageproc` |
| `video::create_background_subtractor_mog2` | **the one real port** — Zivkovic adaptive GMM, ~150 lines of per-pixel math | — |

Nine of the ten are a swap to crates we **already depend on**. Only MOG2 is a genuine
reimplementation.

## What we get

- **Cross-compiles anywhere.** Windows (#80), ARM (a Pi/NAS appliance), musl — all become a plain
  `cargo build --target …`, no vcpkg, no libclang, no `LIBCLANG_PATH`/`DYLD_FALLBACK_LIBRARY_PATH`
  dance, no `deploy/edge-linux/Dockerfile` cross-build container.
- **No more soname coupling.** Today "the binary links OpenCV dynamically, so the run host's OpenCV
  major must match the build host's — build on the distro you run" (`docs/ref/edge.md`). That
  constraint is why Bazzite runs the edge as a self-contained container. It goes away.
- **The edge rejoins the cargo workspace.** It's out of it today *purely* because of OpenCV ("keeps
  the cloud image build pure-Rust").
- Smaller install (`install.sh` stops provisioning OpenCV), faster CI, faster local builds.

## Phase 1 — a pure-Rust MOG2

New `sentinel-edge/src/mog2.rs` (or `app-core`, if sentinel-flow ever wants it): Zivkovic's adaptive
Gaussian mixture, matching OpenCV's `BackgroundSubtractorMOG2` semantics for the parameters we
actually use — `history`, `varThreshold`, `detectShadows=false`.

- Per pixel: up to K (3–5) Gaussians over the 3 BGR channels, each with weight, mean, variance.
- Per frame: match the pixel against each Gaussian (Mahalanobis vs `varThreshold`), update the
  matched one with learning rate `α = 1/history`, decay the others, replace the weakest on no match.
  Foreground = no match against the background-ranked mixture.
- Ship it behind the **existing** `MotionDetector` interface (`motion.rs`, from #77) so `frames.rs`
  doesn't change: `MotionDetector::new(Params)` + `feed(&frame) -> Verdict`.

**This is the only step with real behavioural risk.** The motion pipeline is calibrated
(`MOTION_FLOOR_PCT` 0.5, `MOTION_MIN_AREA` 300, `MOTION_MOG2_VAR` 16, history 333) and those numbers
mean something only against OpenCV's exact MOG2. Verify empirically, not by inspection — see below.

## Phase 2 — swap the rest

In `frames.rs` + `motion.rs`, replace the remaining nine calls with `image`/`imageproc`. Frames are
already decoded once by the hub and handed around as packed BGR (`Frame::bgr`), so this is mostly
mechanical:

- decode: `image::load_from_memory(&jpeg).to_rgb8()` — note this yields **RGB**, and the detector's
  `detect_bgr` (from #77) wants BGR. Either keep a BGR buffer (swap on decode) or teach `Frame` to
  carry RGB and give the detector an RGB entry point. Pick one; don't let the two drift.
- motion: RGB→luma, resize to 480×270, MOG2 → mask, `imageproc::morphology::open_mut` (3×3),
  non-zero count, `connected_components` → blobs with area + bbox.
- Drop the `opencv` dependency from `sentinel-edge/Cargo.toml`.

## Phase 3 — reap the coupling

- `install.sh`: stop installing libopencv (apt/dnf/yum/apk/brew).
- Move `sentinel-edge` back **into** the cargo workspace.
- Retire `deploy/edge-linux/Dockerfile`'s cross-build + the self-contained runtime container for
  Bazzite — a plain static-ish binary now runs on an immutable host directly.
- `Taskfile.yml`: drop `LIBCLANG_PATH` / `DYLD_FALLBACK_LIBRARY_PATH` from `task edge`.
- Update `docs/ref/edge.md` (the OpenCV section + the Linux/Fedora build section).

## Verification (do not skip — this is a perception change)

Correctness here is not "it compiles", it's "it still sees people".

1. **Golden-frames test**: capture a few hundred consecutive JPEG frames from a real camera (a
   person walking through, and an empty scene). Run both detectors over the sequence offline and
   compare, frame by frame: foreground %, blob count, largest bbox. They will not be bit-identical —
   agree on a tolerance (e.g. the *qualifies* verdict must match on ≥95% of frames, and never
   disagree for more than 2 consecutive frames).
2. Same-scene A/B on the test box: old binary vs new, `MOTION_DEBUG=1`, compare the live peak-score
   traces.
3. Re-run #77's tracking check (walk in → stand still 60s → walk out = exactly one entered + one
   exited), since the motion gate now keys off this new detector's verdict.
4. Idle CPU should *drop* (no OpenCV, no `Mat` allocations) — confirm with the #62 metrics panel.

## Open decisions (grill before building)

1. **Port MOG2, or replace it with something simpler?** A running-average background + adaptive
   threshold is far less code and probably good enough for "did something move" — but it's a
   different sensor, and every tenant's sensitivity threshold (`OnEvent("motion") → If score >= …`)
   is calibrated against today's numbers. Porting MOG2 faithfully keeps existing tenants' graphs
   working unchanged, which I'd weight heavily. **Leaning: faithful port.**
2. **Greyscale or 3-channel MOG2?** OpenCV's runs on the 3-channel frame we hand it. Greyscale is
   ~3× cheaper and usually indistinguishable for motion — but it changes the scores. Measure on the
   golden frames before choosing.
3. **Where does the model live** — `sentinel-edge/src/mog2.rs`, or `app-core` (where a cloud-side
   motion node could reuse it)? Leaning edge-local until something else wants it.
4. **Ship behind a flag first?** `MOTION_ENGINE=opencv|rust` for one release, so a box can be
   flipped back without a rollback. Cheap insurance on a perception change; costs us keeping the
   OpenCV dep for one more version.

## Out of scope (v1)

- Any change to what the edge emits. The motion event shape (`{camera_ip, score, area, blobs, bbox}`)
  and its thresholds stay exactly as they are — that's the whole point.
- The YOLOX detector (already pure Rust via tract).
- Windows packaging — that's [[edge-windows]] (#80), which this unblocks.
