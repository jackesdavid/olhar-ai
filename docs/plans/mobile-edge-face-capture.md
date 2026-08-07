# Mobile as a real edge — the phone camera as a face-capture source

**Board card:** #175 Mobile as a real edge — device camera as a face-capture source
**Goal:** Let the phone act as a genuine edge appliance whose *own camera* is a capture source, so
faces can be enrolled and recognised from it like any other camera.
**Sequenced after #174** — the face pipeline should stop moving before a new capture source is added
to it.

## The premise, and it is worth stating as a hypothesis

The theory is that **a phone camera captures faces far better than a surveillance camera**, and it is
a good theory:

- a phone sensor is close to the subject; a ceiling camera is 3–8 m away
- the face fills a large share of the frame, so `face_width` is large where it matters
- it is held roughly at face height, so `yaw`/`pitch` stay near zero — the pose range the embedder
  was trained on
- autofocus on a near subject, versus a fixed-focus lens set for a room
- no IR-cut washout, no night-mode smearing

Everything #126 measures — sharpness, pose, face width — should be dramatically better. **That is
testable now**: the pipeline records `blur`, `yaw`, `pitch`, `roll` on every capture, so a phone
source and a camera source can be compared on the same axes rather than argued about.

**Do that comparison first.** It costs a handful of captures and decides how much the rest is worth.

## Why this is not just "another camera"

The edge already runs on iOS (#144 — `sentinel-edge` + GStreamer inside the phone). What is missing
is that the device's **own** camera is not a source: the edge speaks RTSP to LAN cameras, and the
phone camera is a local capture device, not an RTSP endpoint.

Two distinct pieces:

1. **Capture** — get frames from `AVFoundation` into the same frame hub the RTSP path feeds, so
   motion gating, YuNet detection and the quality metrics all work unchanged downstream.
2. **Presence** — a phone is not always on, not always pointed anywhere, and moves. It cannot be
   modelled as a fixed camera with a fixed FOV on the map.

## Phases

### Phase 0 — Prove the premise
Capture the same person from a wall camera and from a phone, store both, and compare `blur`,
`face_width`, `yaw`, `pitch`. If the phone is not clearly better, the rest of this card is optional.

### Phase 0.5 — Stream URL as a source — **SHIPPED** (MR !50)

Phase 0 needs phone captures to compare against, and there is a way to get them that does not
require Phase 1 at all: **let a camera be added by RTSP URL** instead of by ONVIF discovery. A phone
running any RTSP app then becomes an ordinary camera, and every metric Phase 0 wants is recorded by
the same pipeline that records the wall camera's — same detector, same alignment, same columns.

The edge needed **no change**, which is why this was hours rather than weeks:

- `resolve_rtsp` already skips ONVIF when `rtsp_url` is non-empty
- `probe_device` already prefers a cached URL over resolving one
- the config-pull carry-forward already preserves a cached URL
- the IP-change reconcile only touches cameras with an ONVIF id, so a hand-added source is never
  reset by it

The only missing piece was a way to DELIVER the URL. It rides the existing one-shot, memory-only
credential channel rather than a new one, because an RTSP URL routinely embeds credentials.

This also works for the edge running ON the phone (#144): it can consume `rtsp://127.0.0.1/…` from
an app on the same device, with no network hop at all.

### Phase 1 — Device camera as a source
Feed `AVFoundation` frames into the existing frame hub. Everything downstream (motion gate, YuNet,
`face_quality`, the `face_detected` event) should need no change — that is the test of whether the
integration is in the right place.

Phase 0.5 makes this **optional rather than blocking**. It is still the better product — no second
app, no URL to copy, no RTSP server draining the battery — but the measurement no longer waits on
it, so Phase 1 can be judged on product merit instead of urgency.

### Phase 2 — Enrollment flow
The obvious first product use: **enroll a person by pointing the phone at them**. Better than
uploading a photo, because the capture goes through the same detector, alignment and quality gate as
recognition will — so the enrolled embedding matches the conditions it will be matched against.

### Phase 3 — The presence model
Decide what a phone *is* on the map and in the device list. A camera that appears and disappears,
has no fixed FOV, and moves between sites breaks assumptions the map and camera list currently make.
**This is the design question of the card**, not the capture code.

## Open decisions

1. **Foreground only, or background capture?** iOS background camera access is heavily restricted;
   assume foreground-only until proven otherwise.
2. **Does the phone run the full edge, or a capture-only mode?** Full edge means motion gating and
   detection on the phone's battery; capture-only means streaming frames to a real edge.
3. **Android.** The iOS edge exists; Android does not. Scope this to one platform first.
4. Whether enrollment-by-phone (Phase 2) should ship *before* general capture — it is the smaller,
   more valuable half and does not need the presence model resolved.

## Out of scope
- Replacing fixed cameras. This adds a source; it does not remove one.
- Live streaming the phone camera to other viewers.

## Related
[[faces-completion]] · [[ios-gstreamer-edge]] · [[light-edge-phone-face-capture]] ·
[[face-quality-gate-upstream]] · [[mobile-live-player]]
