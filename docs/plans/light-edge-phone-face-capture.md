# Light Edge — iPhone as an on-demand face-capture edge — Implementation Plan

**Board card:** #142 light edge — iPhone on-demand face capture
**Goal:** turn the phone (front camera) into a *temporary* edge that captures faces only while its
capture screen is open — foreground-only, so it sidesteps iOS's background-execution limits — and
feeds them straight into the existing embed → recognize → StoreFace pipeline.

## Why this shape (and why not GStreamer)

The phone-as-24/7-edge is an **Android** story (GStreamer, RTSP ingest of *other* cameras — card
#138). This is a **different product**: the phone captures faces from its **own** front camera, on
demand, while the user is looking at it. iOS fully allows foreground camera + compute; it only kills
*background* work. So the whole GStreamer/go2rtc/RTSP machinery is **not needed here** — the phone
already has the frame, and iOS **Vision** (`VNDetectFaceLandmarksRequest`) does face detection +
5-landmark output natively, better than porting YuNet.

**The ingest contract already exists.** `sentinel-control/src/http.rs:532` `face_push` (route `POST
/config/face`, box-token auth via `box_tenant`) accepts exactly what a phone produces —
`{ camera_ip, bbox (xyxy), confidence, face_ratio, kps (5 landmarks), frame_b64 }` — and enqueues a
`face_detected` event (`app_core::work_queue::enqueue_event_blocking`). That event drives the
existing graph (embed via the AI sidecar → `faces::recognize` → StoreFace). **Nothing about the
cloud face pipeline has to change.** The phone is just a new *producer* of `face_detected`.

**The "active only during capture" behaviour is already how the engine works.** A workflow bound to
the phone's camera triggers on `When Event(face_detected)`; no events → idle; phone pushing → it
fires. The user's mental model maps 1:1 onto the event-driven engine — no new "activation" concept.

## Phase 1 — the phone registers as a light-edge box

The phone needs a **box token** (that is what `face_push`'s `box_tenant` authenticates) and a stable
camera identity so a workflow can bind to it.

- **Registration path.** Unlike the appliance (`sentinel-edge/src/enroll.rs` — a stranger types a
  pairing code into the box's local page, claimed via `POST /enroll` → `store::claim_pairing` →
  `rotate_box_token`), the phone **already has the user's session** (the app is logged in). So add a
  **user-authenticated self-register**: `POST /boxes/self-register` (UserClaims) → creates a
  `tenant_boxes` row with `platform = "ios-lite"` (col from migration 0034) and mints a box token via
  the same `new_box_token()` / `rotate_box_token` internals — no pairing-code dance. The token is
  stored in the phone keychain. Reuses `create_pairing`/`rotate_box_token` in
  `sentinel-control/src/store.rs`.
- **Camera identity.** The phone picks a stable `camera_ip` = `phone-<installId>` and, on
  self-register (or first push), **upserts a `devices` row** (name e.g. "iPhone — front camera") so
  the workflow builder and Faces UI can bind to and label it. This is the one small cloud addition;
  everything downstream treats it as any camera.

## Phase 2 — the capture screen (sentinel-mobile)

Foreground-only React Native screen; closing it stops all capture (the iOS-friendly boundary).

- **Camera + detection.** Front camera via `react-native-vision-camera` + its face-detector frame
  processor (wraps iOS Vision), or a thin native module calling `VNDetectFaceLandmarksRequest`
  directly. Output per detected face: bbox, 5 landmarks (mapped to the `kps` flat `[x0,y0,…]` order
  `face_push` expects, source pixels — see `#122`), confidence, and a JPEG crop.
- **Throttle + quality gate on-device** (mirror the edge's discipline, memory
  `storeface-pure-sink`): only push a face above a confidence / face-ratio floor, rate-limited, so
  the pipeline isn't flooded — the *quality decision stays upstream of StoreFace*.
- **Push.** `POST /config/face` with the box token + `frame_b64` = the crop. Reuse the app's API
  layer (`sentinel-mobile/src/…`).
- **UX.** A clear "Capturing…" state, a running count of faces sent, and a stop control. Matches the
  app/site design tokens (memory `mobile-design-parity`).

## Phase 3 — bind a workflow to the phone camera

- The user enables a graph bound to the phone's `camera_ip`: `When Event(face_detected)` → (embed) →
  `RecognizeFace` → `StoreFace`, or a **seeded template** (memory `global-workflow-templates`,
  `seed_templates.rs`) named e.g. "Phone face enrollment". It runs only while the phone pushes.
- Confirm the `When Event` camera filter matches the phone's `camera_ip` (arbitrary string is fine —
  the event carries `camera`); the phone camera showing as a device (Phase 1 upsert) is what makes it
  selectable in the builder.

## Open decisions

1. **Self-register vs. reuse pairing code.** Recommended: user-authenticated `POST
   /boxes/self-register` (the phone has a session; the pairing-code UX exists only because the
   appliance has none). Simpler, one tap. Fallback: reuse `/enroll` + a code shown in the web app.
2. **Native module vs. `react-native-vision-camera`.** The library is faster to ship and already
   wraps Vision; a bespoke native module gives tighter control over the crop/landmark mapping. Start
   with the library; drop to native only if landmark alignment (`#122`) needs it.
3. **Crop vs. full frame in `frame_b64`.** `face_push` takes a frame and the graph re-crops; sending
   the tight crop is lighter over cellular but the graph's detect-fallback (memory
   `face-pipeline-architecture`) may expect a fuller frame. Decide by testing recognition quality on
   crop-only.

## Out of scope (v1)

- Background / 24-7 capture on iOS (blocked by the OS — that is the Android edge, #138).
- Ingesting **other** (RTSP) cameras from the phone — that is the GStreamer edge, not this.
- Android build of this capture screen (Vision is iOS; Android would use ML Kit — a later parity
  pass).
- Live video from the phone camera (this is face events only, not an MSE stream).
