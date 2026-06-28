# Motion-Triggered Stream Recorder — Implementation Plan

**Board card:** Motion-triggered stream recorder (ToDo)
**Goal:** Record motion-triggered video clips on the edge (with a 5-second pre-roll ring buffer), trigger via a workflow node, and play them back in the web app.

---

## Architecture split (by design)

| Layer | What lives here |
|---|---|
| Edge (always-on, outside workflows) | 5-second ring buffer per camera (ffmpeg segment muxer), clip storage, file-serving HTTP routes |
| Workflow node (`RecordStream`) | Trigger: POSTs to edge to start/extend a recording; edge owns the cooldown and finalization |
| Control + DB | `recordings` metadata table, clip manifest push from edge, proxy endpoint for web download |
| Web UI | Recordings list page, `<video>` player |

---

## Phase 1 — Edge ring buffer + clip infrastructure

**New file: `sentinel-edge/src/recording.rs`**

- On startup, spawn one **ring-buffer process per camera** (skips cameras with no active stream):
  ```
  ffmpeg -rtsp_transport tcp -i rtsp://127.0.0.1:8554/cam_<ip>
         -f segment -segment_time 1 -reset_timestamps 1
         -segment_wrap 10
         /var/lib/sentinel/ring/<ip_slug>/%05d.ts
  ```
  Keeps the last 10 × 1-second TS segments on disk (≤ 10 seconds of pre-roll; user asked for 5 s, ring stores slightly more for safety).

- **Per-camera recording state machine**: `IDLE → RECORDING → IDLE`

  - `start(ip)` — if IDLE: select the 5 newest ring segments + spawn ffmpeg concat → MP4 and move to RECORDING; if already RECORDING: no-op, return active `clip_id`.
  - `stop(ip)` — send SIGTERM to the ffmpeg process, await exit, compute duration+size, push metadata to control, return to IDLE.
  - Clip naming: `<ip_slug>_<RFC3339_start>.mp4` in `EDGE_RECORDINGS_DIR` (env default: `/var/lib/sentinel/recordings/`).
  - No cooldown timer on the edge — the workflow is responsible for calling `stop` at the right moment.

- **ffmpeg concat recipe** (ring segments + live tail):
  ```
  ffmpeg -f concat -safe 0 -i ring_concat.txt
         -i rtsp://127.0.0.1:8554/cam_<ip>
         -filter_complex "[0:v][1:v]concat=n=2:v=1[out]"
         -map "[out]" -c:v libx264 -preset ultrafast
         <output.mp4>
  ```
  The concat file lists the 5 selected TS segments; the second input streams live until ffmpeg is signalled.

- **Rotation**: when total size in `EDGE_RECORDINGS_DIR` exceeds `EDGE_RECORDINGS_MAX_MB` (default: 1000 MB), delete oldest clips first.

- **Ring buffer config is pulled from control per box** (not env vars — see Phase 2b). On startup and on each config reload, the edge fetches its recording config and (re)starts ring buffer processes accordingly. Env vars (`EDGE_RECORDINGS_DIR`, `EDGE_RING_TMPDIR`) remain for paths only — everything else is in the DB.

---

## Phase 2 — Edge HTTP routes

**Extend `sentinel-edge/src/http.rs`**

```
POST /recording/start/{ip}
  → {clip_id, status: "started"|"active"}   (idempotent: noop if already recording)
  → 404 if camera not found / no ring buffer active

POST /recording/stop/{ip}
  → {clip_id, duration_secs, size_bytes}    (finalizes clip, pushes metadata to control)

GET /recordings
  → [{ip, clip_id, duration_secs, size_bytes, started_at}]

GET /recording/{clip_id}
  → binary MP4 (streamed)
```

The edge holds no cooldown state — that timer lives entirely in the workflow graph.

The tailnet routes these through `tailscale serve` exactly like snapshots today.

---

## Phase 2b — Per-box ring buffer config (control + settings UI)

**Why per-box:** different edge machines have different storage, CPU, and codec support. An RPi needs `h264_v4l2m2m` and 200 MB limit; an x86 NUC can do `libx264` with 2 GB. Configured once per box in the Sentinel UI, pulled by the edge on boot.

**DB — extend `tenant_boxes` or add `box_recording_config` column (migration 0030):**
```sql
ALTER TABLE tenant_boxes ADD COLUMN recording_config JSONB NOT NULL DEFAULT '{}';
-- shape: { pre_roll_secs, max_storage_mb, codec }
```

**Control endpoint (edge pulls on boot + reload):**
```
GET /config/recording
  → { pre_roll_secs: 5, max_storage_mb: 1000, codec: "libx264" }
```
Same long-poll pattern as `/config/cameras` — edge fetches on startup and after each config reload cycle.

**Settings UI — extend the box detail panel** (`sentinel-app/src/routes/settings/boxes/`):
- Pre-roll seconds (number input, default 5)
- Max storage MB (number input, default 1000)
- Video codec (select: `libx264` / `h264_v4l2m2m` / `h264_nvenc` / `h264_vaapi`)

**Edge startup:** after pulling recording config, start/restart ring buffer processes for each camera using the fetched `pre_roll_secs` and `codec`. On config change (box settings saved in UI), the existing config reload path (`POST /reload`) triggers a ring buffer diff + restart.

## Phase 3 — DB migration + control endpoints

**`crates/app-core/migrations/0030_recordings.sql`**
```sql
CREATE TABLE recordings (
    id           BIGSERIAL PRIMARY KEY,
    tenant_id    UUID NOT NULL,
    camera_ip    TEXT NOT NULL,
    clip_id      TEXT NOT NULL,        -- edge-local filename slug (unique per edge)
    duration_secs INTEGER NOT NULL DEFAULT 0,
    size_bytes   BIGINT NOT NULL DEFAULT 0,
    started_at   TEXT NOT NULL,        -- RFC3339
    created_at   TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
);
CREATE INDEX recordings_tenant_camera ON recordings (tenant_id, camera_ip, started_at DESC);
```

**Extend `sentinel-control/src/domain/mod.rs`** (new `recordings` sub-module):
- `POST /config/recordings/clip` — edge pushes finalized clip metadata; control inserts into DB.
- `GET /recordings?camera_ip=<ip>&limit=50` — list clips for tenant (auth-gated).
- `GET /recordings/{id}/download` — proxy bytes from edge via tailnet (`reqwest` stream through axum body).

---

## Phase 4 — Workflow nodes: `StartRecording` + `StopRecording`

**The cooldown/debounce is entirely in the graph.** The edge only knows start and stop — no timer on the edge side. This lets each workflow implement its own strategy (simple wait, motion-gated loop, alert-triggered stop, etc.).

**`crates/app-core/src/workflows/domain.rs`**
- Add `StartRecording` and `StopRecording` to `NodeKind` enum.
- `START_RECORDING_IN`: `camera` (Camera, required)
- `START_RECORDING_OUT`: `ok` (Bool), `clip_id` (Text) — idempotent: if already recording for this camera, returns the active `clip_id` and keeps going.
- `STOP_RECORDING_IN`: `camera` (Camera, required), `clip_id` (Text, optional — for disambiguation)
- `STOP_RECORDING_OUT`: `ok` (Bool), `clip_id` (Text), `duration_secs` (Num)

**`crates/app-core/src/workflows/node.rs`**
- `run_start_recording`: POST to `{edge}/recording/start/{ip}` → `{clip_id, already_active}`.
- `run_stop_recording`: POST to `{edge}/recording/stop/{ip}` → edge finalizes clip + pushes metadata to control → returns `{clip_id, duration_secs}`.

**Edge API (simplified — no cooldown_secs param):**
```
POST /recording/start/{ip}   → {clip_id, status: "started"|"active"}
POST /recording/stop/{ip}    → {clip_id, duration_secs, size_bytes}
```

**`StartRecording` and `StopRecording` accept any trigger — the graph decides everything.**

Example graphs (all valid, no code changes between them):

```
// Motion-gated with cooldown loop
OnEvent("motion") → StartRecording(camera)
  → Wait(15s) → MotionWatch(camera) → If(moved): ↩ Wait
                                    → If(!moved): StopRecording → SendTelegram

// Telegram bot remote control
OnTelegramMessage("record start") → GetCameraByName(config) → StartRecording
OnTelegramMessage("record stop")  → GetCameraByName(config) → StopRecording → SendTelegram(clip link)

// Fixed-duration clip (no motion check)
OnEvent("motion") → StartRecording(camera) → Wait(30s) → StopRecording

// Multi-trigger: motion starts, door sensor or bot stops
OnEvent("motion")          → StartRecording(camera)
OnEvent("door_opened")     → StopRecording(camera)
OnTelegramMessage("stop")  → StopRecording(camera)
```

`StartRecording` is idempotent (returns the active `clip_id` if already running). `StopRecording` is a no-op if the camera isn't recording. Both nodes are completely passive — zero logic outside the graph.

---

## Phase 5 — Web UI: Recordings page

**`sentinel-app/src/routes/recordings/+page.svelte`** (new route)

- List clips grouped by camera, newest first.
- Each row: camera name, timestamp, duration, size, "Play" + "Download" buttons.
- `<video controls src="/api/recordings/{id}/download">` inline player (control proxies from edge).
- Filter by camera (dropdown) + date range (date picker).
- Add "Recordings" nav item (lock behind `recordings.read` permission — same RBAC as others).

**`sentinel-app/src/lib/data.ts`** — add `listRecordings(cameraIp?, limit)` and `recordingDownloadUrl(id)`.

---

## Resolved decisions

1. **Live notification of new clips** → **Yes.** When edge pushes a finalized clip to `POST /config/recordings/clip`, control inserts the DB row and publishes a `"recording-ready"` SSE event (`{event:"recording-ready", camera_ip, id, duration_secs}`). The recordings page subscribes via `onEvent("recording-ready")` and prepends the new row without a full reload.

2. **Hardware codec** → **Yes, configurable.** `EDGE_RECORDINGS_CODEC` env var (default: `libx264`). Document common values: `h264_v4l2m2m` (RPi/ARM SBC), `h264_nvenc` (NVIDIA), `h264_vaapi` (Intel). The ring buffer segments use the same codec var so the concat step doesn't need transcoding.

3. **Playback delivery** → **Control proxy.** `GET /recordings/{id}/download` on control streams bytes from the edge via tailnet using `reqwest` streamed into axum response body. Uniform, works off-LAN, no browser Tailscale requirement.

4. **Separate tmpdir for ring buffer** → **Yes.** `EDGE_RING_TMPDIR` (default: `/tmp/sentinel_ring`) for the rolling TS segments — fast local tmpfs, lost on reboot (acceptable; ring is ephemeral). `EDGE_RECORDINGS_DIR` (default: `/var/lib/sentinel/recordings`) for finalized MP4 clips — persistent across restarts.

5. **Ring buffer on camera re-discovery** → **Restart the buffer.** The camera reload cycle (triggered by `POST /reload` or the 25-second config poll) diffs the new IP list against the running `HashMap<ip, RingHandle>`. Removed IPs: kill ffmpeg + drop handle. Added IPs: spawn new ring process. Unchanged IPs: leave running. This is the same diff pattern as go2rtc config regeneration.

---

## Out of scope (v1)

- Cloud-side re-encoding / transcoding (clips stay as-is from edge).
- Continuous (non-motion-gated) recording.
- Multi-camera synchronized clips (one clip per camera per trigger).
- Thumbnail generation for the recordings list (can be added as a background job later).
- Clip sharing / public URLs.
- S3/object-store upload (defer until cloud storage budget is defined).
