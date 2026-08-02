# Capture watchdog condemns every restart — Implementation Plan

**Board card:** #164 Capture watchdog kills each fresh pipeline — cameras never recover
**Goal:** let a camera that stalls for more than 12 seconds come back on its own.

## Symptom

Reported 2026-08-02: "we keep losing access to the cameras." Camera `192.168.0.7` (Tapo C100,
tenant Default) reads **Not responding** in the app while it is plainly on the LAN and live in the
Tapo app. Edge logs show a restart storm:

```
09:04:47.969  capture-decoder[192.168.0.7]: gstreamer-linked: no frames within watchdog window — restarting
09:04:47.969  capture-decoder[192.168.0.7]: codec probe → H264
09:04:51.469  capture-decoder[192.168.0.7]: gstreamer-linked: no frames within watchdog window — restarting
09:04:51.469  capture-decoder[192.168.0.7]: codec probe → H264
09:05:19.471  capture-decoder[192.168.0.7]: gstreamer-linked: no frames within watchdog window — restarting
```

`codec probe → H264` succeeding on every attempt is the tell: RTSP DESCRIBE works, the camera
answers, the codec is read. The pipeline is being killed anyway.

## Cause

`cam.latest` holds the freshest JPEG **and is never cleared** — deliberately, so viewers keep an
image while a decoder restarts (module docs, `latest_fresh`). The watchdog read that same field:

```rust
let stalled = match &*cam.latest.lock().unwrap() {
    Some((_, at)) => at.elapsed() > Duration::from_secs(12),  // ← a frame from a PREVIOUS session
    None => started.elapsed() > Duration::from_secs(20),      // ← only if one never arrived
};
```

Once a camera has ever delivered a frame, `latest` is `Some` for the process's lifetime. So after
any stall longer than 12 seconds, **every subsequent run is declared stalled on its first loop
iteration** — the check runs before `bus.timed_pop`, so the new pipeline is torn down before it can
produce anything. The 20-second warm-up grace is reachable only by a camera that has never worked.

Two things make it worse:

1. **The supervisor's backoff never resets.** It resets only after a run lasting ≥30s, and these
   runs die in milliseconds — so backoff climbs to its 30s ceiling and stays there. The 3.5s and
   28s gaps in the log above are the climb.
2. **Each restart dials a fresh RTSP session**, and a consumer camera caps concurrent sessions at
   2–4 (the ONE-PIPELINE invariant at the top of `capture.rs`). The restart storm exhausts the very
   slots recovery needs. The recovery mechanism is what prevents recovery.

This also explains the production history: `camera_online` events carry `offline_secs` of 948,
1079 and **25037** (7 hours). Recovery arrived when the edge process restarted — which recreates
`Cam` and resets `latest` to `None` — not because the pipeline healed.

## Fix

Extract the decision into `is_stalled(last_frame, run_started, now)` and ignore any frame older
than the current run:

```rust
match last_frame {
    Some(at) if at >= run_started => now.duration_since(at) > STALL_AFTER, // 12s
    _ => now.duration_since(run_started) > WARMUP,                          // 20s
}
```

A frame from a previous session no longer counts, so each run gets the same warm-up grace as a
camera being seen for the first time. `cam.latest` keeps its "last known image" role untouched.

Five unit tests cover it, including the regression directly: a 10-minute-old frame with a
1-second-old run must **not** be stalled.

## Not fixed here

- **The restart still costs an RTSP session.** If a camera's session slots are already exhausted,
  the first run after this fix may still fail; it will now be given 20s to try, and the backoff
  will space attempts out. Whether the teardown fully releases the session before the next dial is
  a separate question worth measuring.
- **Backoff reset threshold.** A run must last 30s to be "healthy". With a 20s warm-up that is
  tight — a camera that takes 15s to warm up and then streams for 20s never resets its backoff.
  Left alone until the primary fix is observed in production.

## Verification

Unit tests pass (49 edge tests total). **The real check is the box**: after `v0.5.72` reaches it,
`192.168.0.7` should recover on its own after a stall instead of requiring an edge restart, and
`camera_online.offline_secs` should stop showing multi-hour values.
