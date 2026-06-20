# Presence: Approximate Position from Detection BBox — Plan

**Board card:** Presence: bbox-based detection positioning (ToDo)
**Goal:** Use the bounding box of a detection to place the map pulse at a more accurate position within the camera's FOV cone, rather than always dropping it at 55% of range dead-center.

---

## Background & geometry

`PresenceStore.locate()` currently projects a pulse to 55% of the camera's range, directly along the camera's centre yaw (`cam_rot + room_rot`). This treats every detection as "somewhere ahead" regardless of where in the frame the person actually appeared.

A surveillance camera has known `fov` (degrees), `range` (map units), and `rot` (yaw). If we know where the bounding box is in the frame, we can compute:

1. **Horizontal angle offset** — how far left/right of centre the detection is:
   ```
   cx_norm = (x1+x2) / (2 * frame_w)          // 0 = left edge, 1 = right edge
   angle_offset_deg = (cx_norm - 0.5) * fov    // −fov/2 .. +fov/2
   a = (cam_rot + room_rot + angle_offset_deg) * π/180
   ```

2. **Depth approximation** — for a camera mounted high and angled forward, objects **lower** in the frame are **closer** to the camera (larger `cy` → nearer):
   ```
   cy_norm = (y1+y2) / (2 * frame_h)           // 0 = top of frame, 1 = bottom
   depth = (1 − cy_norm) * range               // bottom = 0 (near), top = range (far)
   ```
   This approximation is valid for cameras with a downward tilt. A flat/horizontal camera reverses the depth relationship — see Open Decisions.

3. **Final world position:**
   ```
   x = camX − sin(a) * depth
   z = camZ + cos(a) * depth
   ```

Both `detect_x` (normalised horizontal, -0.5..0.5) and `detect_depth` (normalised, 0..1) are stored in the presence row so the frontend can recompute with the actual map camera parameters.

---

## Phase 1 — DB migration: add detection position columns

**File:** `crates/app-core/migrations/0029_presence_detect_position.sql`

```sql
ALTER TABLE presence
  ADD COLUMN detect_x     REAL,   -- normalised horizontal centre (-0.5 left .. 0.5 right)
  ADD COLUMN detect_depth REAL;   -- normalised depth (0 near camera .. 1 far/range)
```

Nullable — rows without bbox info keep the current centre-ahead behaviour.

---

## Phase 2 — RecordPresence node: add bbox + frame_size inputs

**File:** `crates/app-core/src/workflows/domain.rs`

Extend `RECORD_PRESENCE_IN` with three optional ports:
- `bbox` — `NumArray` — `[x1, y1, x2, y2]` in source-image pixel coords (from `BreakPersonDetection`)
- `frame_w` — `Num` — frame width in pixels
- `frame_h` — `Num` — frame height in pixels

**File:** `crates/app-core/src/workflows/node.rs` — `run_record_presence`

When `bbox` + `frame_w` + `frame_h` are all present, compute and store:
```rust
let cx = (bbox[0] + bbox[2]) / 2.0 / frame_w − 0.5;  // detect_x
let cy = (bbox[1] + bbox[3]) / 2.0 / frame_h;          // for depth
let depth = 1.0 − cy;                                   // detect_depth
```
Pass `Some(cx)`, `Some(depth)` into `presence::create` and the `ToolUiEvent::Presence`.

**File:** `crates/app-core/src/presence/mod.rs` + `repo.rs`

Add `detect_x: Option<f32>` and `detect_depth: Option<f32>` to the `Presence` struct and `repo::insert` / `repo::load_recent`.

---

## Phase 3 — CameraSnapshot / detection nodes: expose frame dimensions

To wire `frame_w`/`frame_h` without manual config, the nodes that produce images should output them.

**File:** `crates/app-core/src/workflows/node.rs`

- `run_camera_snapshot` / `run_get_current_frame`: add `width` (Num) and `height` (Num) outputs from the decoded JPEG dimensions (use `image::guess_format` + `image::load_from_memory` header, or just `jpeg_decoder::Decoder::dimensions`).
- `run_person_detection` + `run_object_detection`: also output `width` + `height` from the original input image (already in scope).

**File:** `crates/app-core/src/workflows/domain.rs`

Add `width`/`height` ports to the output definitions of `CameraSnapshot`, `GetCurrentFrame`, `PersonDetection`, `ObjectDetection`.

---

## Phase 4 — Frontend: use detect_x / detect_depth in placement

**File:** `sentinel-app/src/lib/types.ts`

Add `detect_x?: number | null` and `detect_depth?: number | null` to both `Presence` and `PresenceEvent`.

**File:** `sentinel-app/src/lib/map/presence.svelte.ts` — `locate()`

When `detect_x` and `detect_depth` are present on the event, override the centre-ahead formula:
```ts
const angleOffsetRad = (p.detect_x ?? 0) * ((c.fov ?? 62) * Math.PI / 180);
const dist = (p.detect_depth ?? 0.55) * (c.range ?? 4.2);
const a = (((c.rot ?? 0) + (r.rot ?? 0)) * Math.PI / 180) + angleOffsetRad;
return { x: camX - Math.sin(a) * dist, z: camZ + Math.cos(a) * dist };
```
Falls back to the existing `0.55 * range` centre-ahead behaviour when the fields are absent.

---

## Phase 5 — SSE event: carry detect_x / detect_depth

**File:** `sentinel-flow/src/engine.rs` — `ToolUiEvent::Presence` dispatch

Ensure `detect_x` and `detect_depth` are forwarded through the SSE payload to the frontend (they come from `run_record_presence` → `ToolUiEvent::Presence`).

---

## Open decisions

1. **Depth inversion for flat cameras.** The `(1 - cy_norm) * range` formula assumes a downward-tilted camera. A camera flush with the wall facing horizontally needs the inverse. Options: (a) a `camera_tilt` config field on `RoomCamera`; (b) let users flip the formula with a node-level "invert depth" toggle; (c) document the convention and defer.

2. **Frame dimensions source.** Exposing `width`/`height` from detection/snapshot nodes (Phase 3) is the cleanest path, but it adds ports to existing nodes. Alternative: RecordPresence accepts a pre-normalised `[cx_norm, cy_norm]` Num input (0-1 each) and the user computes it upstream with Num divide nodes. Simpler node graph, more friction in setup.

3. **Multi-person workflows.** When ForEach loops over `PersonDetection[]`, each iteration has its own bbox. RecordPresence inside ForEach should receive that iteration's bbox naturally — no special handling needed, but worth validating.

4. **Accuracy expectations.** This is an approximation — it assumes a pinhole camera with known horizontal FOV and no lens distortion. Results will be directionally correct but not survey-grade. A visual disclaimer on the map pulse (e.g. dotted ring vs solid) could communicate "estimated position".

---

## Out of scope (v1)
- Camera tilt angle as a first-class map parameter
- Sub-pixel / homography-based projection (needs camera calibration data)
- Showing the detection bbox as an overlay on the map
