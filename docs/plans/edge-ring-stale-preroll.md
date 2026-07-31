# Pre-roll can splice stale frames across a stream gap

**Board card:** #101 Ring pre-roll ignores time — stale frames after a gap (Bugs)
**Goal:** A clip's pre-roll is only footage from the seconds immediately before it started — never
frames left in the ring from before a disconnect.

Reasoned out by the owner 2026-07-17: *"the last ring was yesterday… a network issue, a camera turned
off… today a new recording starts, that very old ring will be added to the new recording."* Confirmed
in the code.

---

## Confirmed

`H264Frame { data, is_keyframe }` has **no timestamp**. The ring evicts **by count only**:

```rust
let max_frames = (pre_roll_secs as usize) * 33;   // ASSUMES ~33 fps
ring.frames.push_back(frame);
while ring.frames.len() > max_frames { ring.frames.pop_front(); }
```

The collector **loop-reconnects** (`loop { rtsp_stream_once(...); Err => reconnect in 3s }`) and pushes
post-reconnect frames into the **same** `mem_ring`. So across a camera drop, the deque holds frames
from two real-time epochs with nothing to distinguish them.

Pre-roll then slices by count, no clock:

```rust
let max_frames = (max_pre as usize) * 30;
let win_start = ring.frames.len().saturating_sub(max_frames);  // "last N frames"
// earliest keyframe in [win_start..], else last keyframe overall
```

So the failure the owner described is real: camera drops with the ring full → reconnects → a recording
starts before `max_frames` fresh frames have arrived → the "pre-roll" is **pre-gap frames spliced
directly onto the new footage**. The clip opens with stale video and jumps across the gap with no
seam, no marker, no way for the count window to know. Even a fully-refilled ring is only right by luck
— nothing *enforces* that the N frames are recent, because no frame knows when it arrived.

## The fix: time, not count

- **Stamp each `H264Frame` with an arrival `Instant`** (monotonic — wall clock can jump).
- **Evict by age**: drop frames older than `pre_roll_secs` (keep a small count cap as a memory
  backstop). A ring that's been idle since yesterday is then *empty*, not stale.
- **Pre-roll by time**: the window is "frames arrived within the last `pre_roll_secs`", from the
  earliest keyframe inside it. If nothing recent, no pre-roll — a clean start beats a lying one.
- **Gap reset**: when a frame arrives more than a small threshold after the previous (a reconnect, a
  freeze), clear the ring. A discontinuity makes all prior pre-roll meaningless by definition.

## Interaction with #98

#98 (orphaned rings never stopped) and this are the same subsystem seen from two sides: rings that
outlive their camera, and frames that outlive their moment. Both come from the ring having **no
lifecycle tied to real time** — one for the session, one for the frames. Worth fixing together; the
timestamp this card adds also lets #98's reconcile spot a ring that's gone silent.

## Verification

- Stream, stop the camera, wait well past `pre_roll_secs`, restore it, start a clip immediately →
  pre-roll contains only post-restore frames (likely none, if the clip starts at reconnect), never
  pre-gap video.
- Continuous stream, start a clip → pre-roll is the last `pre_roll_secs` of real footage, unchanged
  from today's good case.
- A clip's first frame timestamp is within `pre_roll_secs` of its start — assertable in a test with a
  synthetic frame feed.

## Out of scope

  enter the pipe.
- The orphaned-ring lifecycle (#98) — sibling, do together, but tracked there.
