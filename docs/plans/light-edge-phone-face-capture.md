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
- **Camera identity.** The phone stamps a stable `camera_ip = phone-<box_id[..8]>` (derived from the
  box id, so unique with no second identifier) on every face event. **DONE (Phase 1):** `POST
  /boxes/self-register` (UserClaims, `devices.write`) → `store::create_box_with_platform(…,
  "ios-lite")` → returns `{ box_id, camera_ip, token }`; the phone stores the token in the keychain.
  No Tailscale key, no pairing code.
- **Why NO `devices` row (discovered reading the code).** `devices` is **full-replaced** by the
  appliance's camera sync — `app_core::devices::repo::save` does `DELETE all-for-tenant + re-insert`
  the edge's list. A phone-camera row injected there would be wiped the next time any appliance in the
  tenant syncs. So the phone camera is identified **purely by its `camera_ip` string** on the face
  events; it does not live in `devices`. Making it a first-class, builder-selectable device would need
  `devices` to carry an origin/box scope so sync only replaces its own box's cameras — that is a
  separate change, tracked as an open decision below, not v1.

## Phase 2 — the capture screen (sentinel-mobile) — DONE (needs a device build to validate)

Foreground-only screen; closing/stopping ends capture (the iOS-friendly boundary).

- **`sentinel-mobile/src/lightEdge.ts`** — the pure-TS core (type-checks clean): `restore` /
  `ensureRegistered` (user JWT → `POST /boxes/self-register`, box token in the keychain via
  `expo-secure-store`) and `pushFace` (box token → `POST /config/face`). Two credentials on purpose,
  mirroring an appliance.
- **`sentinel-mobile/app/(tabs)/capture.tsx`** — the Capture tab. Front camera via **`expo-camera`**;
  while capturing it snaps a photo every ~1.8 s, downscales to 720 px (`expo-image-manipulator`), and
  pushes it. Running count + stop control; unmount stops capture.
- **Camera choice: `expo-camera`, not `react-native-vision-camera` (for v1).** vision-camera + an
  MLKit face-detector frame processor would give on-device auto-detection, but it drags worklets and a
  fragile JS-side crop; `expo-camera` is an SDK-native config plugin with a small surface. So **v1
  sends the full (downscaled) photo and the cloud detects + crops** (`embed_face_crop`'s
  detect+fallback) — which also settles open-decision #4. On-device auto-detection is a later
  enhancement (and the path to Android parity via MLKit).
- **Validation boundary.** TS type-checks; the **native camera cannot be exercised from the host** —
  it needs `npx expo prebuild` + a device build (the Release-to-iPhone flow from #139). That build is
  the only way to confirm expo-camera + the push work end to end.

## Phase 3 — bind a workflow to the phone camera

- The user enables a graph bound to the phone's `camera_ip`: `When Event(face_detected)` → (embed) →
  `RecognizeFace` → `StoreFace`, or a **seeded template** (memory `global-workflow-templates`,
  `seed_templates.rs`) named e.g. "Phone face enrollment". It runs only while the phone pushes.
- The `When Event` camera filter matches the phone's `camera_ip` (arbitrary string is fine — the
  event carries `camera`). Since there is no `devices` row in v1 (see Phase 1), the user binds by
  typing/pasting the `camera_ip` the app shows them after self-register, or the template listens to
  `face_detected` from any camera and the phone is the only producer while capturing. Builder
  picklist selection is open-decision #3.

## Open decisions

1. **Self-register vs. reuse pairing code.** Recommended: user-authenticated `POST
   /boxes/self-register` (the phone has a session; the pairing-code UX exists only because the
   appliance has none). Simpler, one tap. Fallback: reuse `/enroll` + a code shown in the web app.
2. **Native module vs. `react-native-vision-camera`.** The library is faster to ship and already
   wraps Vision; a bespoke native module gives tighter control over the crop/landmark mapping. Start
   with the library; drop to native only if landmark alignment (`#122`) needs it.
3. **Making the phone camera builder-selectable.** v1 binds workflows by the raw `camera_ip` string
   (shown to the user after self-register). To have it appear as a picklist device like a real camera,
   `devices` needs an origin/box_id scope so the appliance sync's full-replace only deletes its own
   box's cameras — a focused migration + `repo::save` change, deferred out of v1.
4. **Crop vs. full frame in `frame_b64`.** `face_push` takes a frame and the graph re-crops; sending
   the tight crop is lighter over cellular but the graph's detect-fallback (memory
   `face-pipeline-architecture`) may expect a fuller frame. Decide by testing recognition quality on
   crop-only.

## Out of scope (v1)

- Background / 24-7 capture on iOS (blocked by the OS — that is the Android edge, #138).
- Ingesting **other** (RTSP) cameras from the phone — that is the GStreamer edge, not this.
- Android build of this capture screen (Vision is iOS; Android would use ML Kit — a later parity
  pass).
- Live video from the phone camera (this is face events only, not an MSE stream).
