# Edge pre-roll rings leak — reconcile them against the device set

**Board card:** #98 Edge ring reconcile — stop rings for cameras that left (Bugs)
**Goal:** A pre-roll ring should exist for exactly the cameras the edge currently streams. When a
camera changes IP, is removed, or its recording ends, its ring stops — instead of retrying a dead
address forever.

Found 2026-07-17: a camera moved .184 → .187 (wifi change). The user re-added it at .187, live works
— but the box log fills with `ring-rtsp[192.168.1.184]: describe: 404` every ~3s, live, against an
address no device uses any more.

---

## Root cause

`RecordingManager` keeps `ring_sessions: HashMap<camera_ip, RingSession>`, each a thread running
`run_rtsp_collector(ip, …)` forever. Two ways in, no way out:

- **`start(camera_ip, …)`** (recording start) calls `start_ring` — and `stop()` **never calls
  `stop_ring`.** `stop_ring` is `#[allow(dead_code)]`; grep confirms it's invoked nowhere. So a ring
  started by any recording outlives it.
- **`warm_cameras()`** (state.rs, on every reload when `cameras_always_warm`) calls `start_ring` for
  each streamable camera. It only ever **starts** — there is no "un-warm" for a camera that left the
  device set.

So `ring_sessions` only grows. A camera that changes IP leaves its old-IP ring orphaned; the thread
can't reach the old address and logs a DESCRIBE 404 on every retry, forever. The .184 ring in this
report came from an earlier recording test against .184; the IP move just made its orphan loud.

`start_ring` is idempotent on the *new* IP (keyed by `camera_ip`), so .187 is fine — nothing cleans
up .184.

## The fix

Reconcile on reload, next to `warm_cameras` (which already walks the current device set):

- After the device list updates, compute the set of current streamable camera IPs.
- For every key in `ring_sessions` **not** in that set, `stop_ring(ip)` — send its stop signal and
  drop the session. That covers IP change (old IP gone), removal, and a camera going un-streamable.
- Keep the existing `warm_cameras` start pass. Together they make `ring_sessions` a mirror of the
  device set, not an append-only log.

Independently, decide the recording-stop case: should a ring outlive its recording at all? Warm mode
wants it to (next recording is instant); non-warm mode arguably shouldn't hold an RTSP session open
after `stop()`. Leaning: in non-warm mode, `stop()` calls `stop_ring`; in warm mode the reconcile
above is what bounds it. Either way the reconcile is the safety net.

## Why not just "restart the box"

That clears it (the immediate unblock is a Boxes-page Restart — a real journey), but it's the exact
"fix the edge by hand" the standing rule rejects. A camera changing IP is a normal event the box must
absorb by itself, like the IP-reconcile it already does for the device list.

## Verification

- Move a camera's IP (or re-add at a new IP): within one reload the old-IP ring stops; no more
  `ring-rtsp[old.ip]` lines. The new IP streams and records.
- Remove a camera: its ring stops.
- Warm mode: rings match the streamable set after any reload — no orphans, no missing.
- A recording still gets its pre-roll (the ring is up before the record window).

## Out of scope

- The duplicate-device question (re-adding a moved camera by hand makes a second row instead of
  reconciling by ONVIF id) — real, but that's device identity, not the ring. Track separately if it
  bites; the empty-id backfill is meant to catch it on the next discovery.
