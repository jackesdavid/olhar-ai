# Edge frame hub — one decode per camera + motion-gated tracking

**Board card:** #77 Edge frame hub — one decode per camera + motion-gated tracking (ToDo)
**Goal:** Cut the edge's steady-state CPU by ~5-10× on an idle scene so the appliance we sell can
be a ~R$1.200 Intel N100 mini PC instead of a workstation — without giving up any live, motion,
detection or tracking capability.

Related: [[edge-resource-monitoring]] (#62 — the CPU/mem panel that will show this working),
[[clip-local-retention]] (#78 — the storage half of the same appliance-sizing question).

---

## Why (the hardware decision behind it)

We want to ship a box with the product. Sizing it exposed three facts about what the edge burns
today, none of which is storage:

| Load | Cadence | Cost (per camera, modern x86 core) |
|---|---|---|
| go2rtc/ffmpeg decodes the substream → `frame.jpeg` | 6 fps | ~5-10% of a core |
| Motion: JPEG decode → resize 480×270 → MOG2 → contours | 6 fps | ~5% of a core |
| **YOLOX-Nano 416×416 (tract, CPU), one thread per job×camera** | **2.5 fps, ungated** | **~30-60% of a core** |

The tracker is the dominant cost and it runs **whether or not anything is happening** — an empty
room at 3am costs exactly as much as a busy shop floor. Meanwhile motion and tracking each fetch
and decode the *same frame* independently, so a tracked camera decodes one JPEG **up to 3×** per
cycle (motion's `imdecode`, `detector::detect`'s internal decode, and `detector::dimensions`'s
second decode when the job has lines/zones).

Fixing both means an N100 (4 cores) comfortably runs 8+ cameras. Nothing here changes what the
product *does* — only what it costs to do it.

## What's true today (grounding)

- `sentinel-edge/src/motion.rs:60` — manager spawns **one watcher thread per camera** with a
  stream. Always on, no gate. Loop at `MOTION_FPS`=6 (`motion.rs:46`): `go2rtc::frame_from` →
  `imgcodecs::imdecode` (`motion.rs:166`) → resize → MOG2 → `find_contours`.
- `motion.rs:235` — `qualifies` (the raw per-frame verdict) is computed every tick, but an event
  is only POSTed once per `MOTION_MIN_INTERVAL_SECS` (4s, `motion.rs:48`). **The throttle is on
  the emit, not on the verdict** — important for the gate below.
- `sentinel-edge/src/tracking.rs:396` — `run_track`'s loop is paced only by `detect_gap` (400ms,
  `tracking.rs:128`). It **never consults motion**. Every cycle: `frame_from` → `detector::detect`
  → `tracker.update`.
- `tracking.rs:421` — when a job has lines or a zone, `detector::dimensions(&jpeg)` decodes the
  same JPEG a **second** time, every cycle, just to read width/height.
- `crates/app-core/src/agent/detector.rs:157` — `preprocess` decodes with the `image` crate, then
  fills the `[1,3,416,416]` **BGR, raw 0-255** tensor with a per-pixel nested loop
  (`detector.rs:172-179`). `detect(&jpeg)` is also the entry point **sentinel-flow** uses on the
  cloud side (`crates/app-core/src/workflows/node/vision.rs`) — it must keep working unchanged.
- Tracker tuning is per-job: `max_age`=25 cycles (~10s at 400ms), `min_hits`=2 (`tracking.rs:46-60`).

## Phase 1 — the frame hub (one fetch, one decode, per camera)

New module `sentinel-edge/src/frames.rs`: **one source thread per camera**, replacing the
per-camera fetch+decode that motion and each tracking job do independently.

The published frame is deliberately **not** an `opencv::Mat` (`Mat` is not `Sync`, so it can't be
shared behind an `Arc` across the worker threads). Publish plain buffers instead:

```rust
pub struct Frame {
    pub seq:   u64,            // monotonic; consumers skip frames they've already seen
    pub jpeg:  Arc<Vec<u8>>,   // still needed for the motion event's frame_b64
    pub bgr:   Arc<Vec<u8>>,   // decoded once, BGR8, w*h*3
    pub w:     i32,
    pub h:     i32,
    pub motion_now: bool,      // this frame's RAW MOG2 verdict (not the throttled emit)
    pub at:    Instant,
}
```

- The source thread does: `frame_from` → `imdecode` → copy `Mat::data_bytes()` into the `bgr`
  buffer → run the existing MOG2 pipeline (it owns the per-camera background model) → set
  `motion_now` → emit the motion event to control when it qualifies **and** clears the 4s throttle
  (the existing `motion.rs` logic, moved, not changed) → publish.
- Publish into a latest-value slot per camera (`Arc<(Mutex<Option<Arc<Frame>>>, Condvar)>`), so a
  slow consumer never backs the source up — it just misses frames, which is correct for live video.
- Consumers subscribe by camera ip and block on the condvar for `seq > last_seen`, with a timeout.
- Reuse `motion.rs`'s existing reconciler shape (`motion.rs:60-91`): one source per camera with a
  resolved `rtsp_url`, started/stopped every 30s against the live device list.
- Consumers rebuild an OpenCV view over the shared `bgr` with `Mat::new_rows_cols_with_data`
  (borrowed, no copy) when they need OpenCV ops.

Result: motion's decode moves into the hub (same total work), and every tracking job on that
camera now gets its frame **for free**.

## Phase 2 — detector: a BGR entry point (kill the extra decodes)

In `crates/app-core/src/agent/detector.rs`:

- Split `preprocess` into `preprocess_bgr(bgr: &[u8], w: u32, h: u32) -> (Array4<f32>, f32)` (the
  letterbox + BGR tensor fill, unchanged in behaviour) and keep `preprocess(image_bytes)` as
  `decode → to BGR → preprocess_bgr`.
- Add `pub fn detect_bgr(bgr: &[u8], w: u32, h: u32) -> Result<Vec<Detection>, String>`.
- `detect(&jpeg)` stays exactly as-is → **sentinel-flow and every workflow vision node are
  untouched.**
- The edge tracker calls `detect_bgr(&f.bgr, f.w, f.h)` and drops the `detector::dimensions` call
  entirely — `f.w`/`f.h` are already known (`tracking.rs:421` goes away).

Per tracked camera per cycle this takes JPEG decodes from **3 → 0** (the hub already did the one).

Optional while in here: the per-pixel nested loop at `detector.rs:172-179` is the hot path of
preprocess. A row-wise fill over the resized buffer is a straightforward win. Measure first; don't
let it block the phase.

## Phase 3 — the motion gate (the big one)

`run_track` (`tracking.rs:396`) stops running YOLOX on a scene where nothing is happening.

**Gate rule — run detection this cycle when ANY of:**

1. `motion_active` — any frame with `motion_now` in the last `hold` seconds (default 3s; must
   exceed the hub's frame period so real activity reads as continuous). Note this keys off the
   **raw per-frame verdict**, not the 4s-throttled emit — gating on the emit would make the gate
   stutter.
2. `tracker.count() > 0` — something is still being tracked. This is what makes the gate safe: a
   person who walks in and then **stands perfectly still** stops producing motion (MOG2 absorbs a
   stationary subject into the background), but YOLOX keeps seeing them, so we keep detecting and
   the track stays confirmed. A naive wall-clock grace period would drop them.
3. `warming_up` — the worker's first ~5s. Seeds the tracker with whatever was **already static in
   frame** when the worker started (a car parked before the box booted never generates motion, so
   without this it would never be detected).
4. `idle_sweep_due` — one detection cycle every `idle_sweep_secs` (default 30s) regardless. Cheap
   insurance (≈0.3% of a core) against anything that appears without tripping motion.

Otherwise: skip the inference, do **not** tick the tracker (an untick can't age a track out, which
is what we want — nothing left the scene without generating motion), and sleep to the next cycle.

**Exit correctness:** a subject leaving *is* motion, so departures always run detection. YOLOX then
returns 0 detections for `max_age` cycles → `Exited` fires → `count()` hits 0 → motion goes quiet →
detection stops. The gate never swallows an exit event.

Escape hatch: `TRACK_MOTION_GATE=0` forces the old always-on behaviour for a scenario we haven't
thought of.

## Phase 4 — cadence defaults

- `MOTION_FPS` 6 → **4** (`motion.rs:46`). Indistinguishable for presence; −33% on the hub's decode
  and MOG2 work.
- `MOTION_MOG2_HISTORY` 500 → **333** (`motion.rs:52`). The history is counted in **frames**, so
  dropping fps without rescaling it would stretch the background model's time constant from ~83s to
  ~125s and change motion behaviour. 333 frames @ 4fps ≈ the same 83s. **Do not skip this.**
- The hub's rate is the motion rate; the tracker samples the latest frame at its own `detect_ms`
  and skips a frame it has already seen (`seq`), so `detect_ms` < the frame period no longer means
  detecting the same frame twice.

## Verification

- `MOTION_DEBUG=1` + `TRACK_DEBUG=1` on the Mac test box: confirm motion still emits at the same
  scores, and that the tracker logs detection cycles **only** while motion/tracks are live.
- Walk-in → stand still for 60s → walk out: exactly one `object_entered` and one `object_exited`.
  This is the regression the gate could plausibly break; test it explicitly.
- Idle empty room for 5 min: near-zero CPU from tracking threads (`top` / the #62 metrics panel).
- Cold start with a person already standing in frame: they must be detected (Phase 3, rule 3).

## Open decisions (grill before building)

1. **Does the hub own motion, or is motion just another subscriber?** Owning it is simpler (one
   thread per camera total, MOG2 state lives with the decode) but fuses two concerns in one module.
   Subscriber-style is cleaner but adds a thread + a buffer copy per camera. Leaning **hub owns it**.
2. **`motion_now` in the frame, or a separate motion-state slot the tracker reads?** In-frame is
   simplest; a separate slot decouples the tracker's cadence from the frame cadence. Leaning in-frame.
3. **Do we still need `cameras_always_warm`?** The hub already keeps the substream hot (it polls at
   4-6 fps), so the flag now only controls the retina pre-roll ring. Worth renaming for honesty.
4. **Should the gate be exposed in the UI at all**, or stay an internal optimisation with an env
   escape hatch? Leaning internal — a user has no way to reason about it.

## Out of scope (v1)

- VAAPI/QuickSync offload of go2rtc's H264 decode + JPEG encode (the next lever if CPU is still
  tight on the N100 — worth a card of its own once we have real numbers from #62).
- Replacing tract with a quantized/NPU backend.
- Any change to what the edge emits: motion events, track events and their payloads stay identical.
