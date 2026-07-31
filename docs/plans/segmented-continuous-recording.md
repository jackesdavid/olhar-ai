# Segmented continuous recording — cap clip size, crash-durable, incremental upload

**Board card:** #87 Segmented continuous recording (ToDo)
**Companion:** [[delay-trigger-recording-signal-flow]] (#86 — the node that opens long sessions)
**Goal:** Split a long recording session into fixed-length, keyframe-aligned, lossless MP4 chunks on the
edge — capping file size, making each finished chunk crash-durable, and letting chunks upload to R2
incrementally.

## Context — why

With the Delay Trigger ([[delay-trigger-recording-signal-flow]]), a long spell of sustained motion (hours)
is one logical session, so `stop` only fires after quiet. Today that would produce **one giant MP4**.
Worse, the current writer moves the moov atom only on a clean `stop()` (`sentinel-edge/src/recording.rs:428`),
so an edge crash mid-recording loses the **entire** clip. Segmenting into fixed-length chunks caps size,
makes each finished chunk crash-durable, and lets chunks upload to R2 **incrementally** instead of one huge
end-of-session PUT.

Segmentation is an **edge + storage + playback** concern, orthogonal to the node: the node governs the
*logical* session (start on first motion, stop after quiet); the edge silently rolls segments *inside* it.


**segment muxer**:
```
-f segment -segment_time <segment_secs> -reset_timestamps 1
-segment_format mp4 -segment_format_options movflags=+faststart
{session_id}_%05d.mp4
```
  GOP. Each segment begins on an IDR → independently decodable (our reassembler already prepends SPS/PPS to
  keyframes; `H264Frame.is_keyframe`).
- Each finished segment gets its moov on rollover → crash only risks the in-progress segment.
- **Segment completion signal:** add `-segment_list <session>.csv -segment_list_type csv` (filename +
  start/end times, appended as each segment finalizes) and tail it — or watch for segment N+1 appearing.
  This drives the per-segment manifest push + R2 upload enqueue. Edits localized to
  `sentinel-edge/src/recording.rs` `RecordingManager::start`/`stop` (args at `recording.rs:307-319`) and the
  relay/finalize path.
  finalizer thread) to reimplement what the muxer already does. Revisit only if size-based caps are needed.

## Per-tenant configurable segment length

`segment_secs` is a **tenant setting** with a sensible default (**600s / 10 min**; clamp ~[60, 3600]).
Surface it in Settings (recording section) and include it in the config the edge already pulls (the
cameras/recording config poll), so the edge applies it on start/reload. A reasonable default means most
tenants never touch it.

## Mandatory schema change — session grouping (migration 0055)

The whole pipeline assumes **one clip = one file = one row = one R2 object** (`clip_id = {ip}-{unix_ts}`, no
grouping). Add grouping:
- `crates/app-core/migrations/0055_recording_sessions.sql` — add to `recordings`: `session_id TEXT` (the
  session's start id) + `segment_index INTEGER NOT NULL DEFAULT 0`. Index `(tenant_id, session_id,
  segment_index)`.
- Thread `session_id` + `segment_index` through: edge `ClipPush` (`sentinel-edge/src/http.rs:195-202`,
  `recording.rs` sidecar) → control `ClipPush`/`push_clip` (`sentinel-control/src/domain/recordings.rs:346-375`)
  → `store::insert_recording` (`store.rs:735-769`).

## R2 upload — research conclusion (no redesign, additive only)

The #64 edge→R2 path (presigned PUT per clip, one-at-a-time bounded queue, key
`{days}d/{tenant}/{yyyy}/{mm}/{clip_id}.mp4`, GB-month sampler, lifecycle-by-prefix retention) already
handles N independent files. Changes needed:
- **Additive:** carry `session_id`/`segment_index` in the manifest; optionally nest the R2 key by session
  (`…/{session_id}/{index}.mp4`) for tidiness. Upload queue + billing sampler **unchanged** (still sums
  `size_bytes` per uploaded object) — segments just enqueue incrementally as they finalize (a win).
- **One behavioral change — delete/purge as a unit:** immediate purge (`clip_store::purge_tenant`) and the
  `work_jobs` delete leg must operate on the whole `session_id` group (iterate its segments), not a single
  `clip_id`. Retention-by-lifecycle-prefix still works per object.

## Playback (#67) — grouping only, no new player

`sentinel-app/src/routes/playback/+page.svelte` already swaps `<video>.src` at clip boundaries and models
clips as blocks on a shared time axis, so contiguous segments play back-to-back seamlessly with no new
machinery. Use `session_id` to render a session as **one logical recording** in the list/timeline (group the
segment blocks). HLS/fMP4 (documented in [[recording-playback-timeline]] as the future scaling substrate)
stays out of scope here.

## Verification

- Edge: start a recording, let it run past `segment_secs` several times; confirm N standard-MP4 files roll
  with no dropped frames across boundaries (concat + frame-count check), each begins on an IDR, and each
  finalizes (moov present) at rollover. Kill the edge mid-segment → confirm all *completed* segments are
  intact (only the in-progress one is lost).
- Control/DB: confirm each segment inserts a `recordings` row sharing `session_id` with an incrementing
  `segment_index`, and each uploads to R2 independently as it finalizes.
- Playback: open the session in `/playback`, confirm the segments render as one grouped recording and play
  seamlessly across boundaries.
- Delete: purge the session, confirm every segment object + row is removed as a unit.

## Out of scope (v1)

- Size-based (vs time-based) segmentation.
- fMP4/HLS/CMAF + hls.js player (the [[recording-playback-timeline]] scaling path).
- Re-muxing/transcoding segments; changing the camera GOP via ONVIF to tighten split granularity.
