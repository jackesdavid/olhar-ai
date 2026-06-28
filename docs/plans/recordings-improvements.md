# Recordings Improvements — Implementation Plan

**Board card:** Recordings: storage, names, filters, delete, sort, paginate
**Goal:** Production-grade recordings library — friendly names, server-side filter/sort/paginate, real delete (disk + DB), camera/room columns, grouping, configurable storage quota.

> Pre-roll + stitch approach is locked in and perfect — do NOT change `recording.rs` concat logic.

---

## What exists today

- Edge: `POST /recording/start`, `POST /recording/stop`, `GET /recording/:clip_id`
- No list endpoint on edge; control has `GET /recordings?camera_ip=&limit=50`
- No friendly names — clip IDs are `<ip_slug>_<RFC3339>.mp4`
- No delete endpoint anywhere
- No server-side sort/paginate/filter
- `max_storage_mb` exists in `RecordingConfig` but isn't surfaced in the UI
- Camera IP stored but no room/camera name lookup

---

## Phase 1 — Friendly clip names (#2)

**Edge (`sentinel-edge/src/recording.rs`)**

Generate a 3-word slug at `start()` time using a small built-in word list baked into the binary (no runtime I/O).

- Word list: two arrays of ~256 adjectives + ~256 nouns (kept short, safe, memorable).  
  Name formula: `<adj>-<adj>-<noun>` e.g. `swift-calm-ember`.
- Clip filename stays `<ip_slug>_<RFC3339>.mp4` (used as the stable `clip_id`).
- `friendly_name` = `swift-calm-ember 23-Jun 14:32` (3 words + `dd-Mon HH:MM` in UTC).
- `ClipMeta` gains `friendly_name: String`; pushed to control with finalization payload.

**Control DB (`crates/app-core/migrations/0030_recordings.sql`)**

Add `friendly_name TEXT NOT NULL DEFAULT ''` column to the `recordings` table.

**Web UI (`sentinel-app/src/routes/recordings/+page.svelte`)**

Show `friendly_name` as the primary label in the recording row (bold); clip_id stays a tooltip.

---

## Phase 2 — Edge list + server-side sort/filter/paginate (#3 #5 #7 #8)

**New edge route: `GET /recordings`**

```
GET /recordings?camera_ip=<ip>&sort=<field>&dir=asc|desc&limit=<n>&offset=<n>
```

- Reads `EDGE_RECORDINGS_DIR`, parses clip filenames for `ip_slug` + `started_at`.
- Loads `ClipMeta` side-car `.json` file per clip (written alongside MP4 at stop time)
  containing: `clip_id`, `friendly_name`, `camera_ip`, `duration_secs`, `size_bytes`, `started_at`.
- **Filter**: `camera_ip` exact match (or empty → all cameras).
- **Sort fields**: `started_at` (default desc), `duration_secs`, `size_bytes`, `friendly_name`.
- **Pagination**: `limit` (default 50, max 200) + `offset`.
- Response: `{ total: N, items: [...] }`.

**Control proxies the list** (`GET /recordings` on control) — proxies to edge, adds `camera_name`
and `room_name` via device lookup before returning. Keeps control as the single source the UI hits.

**`sentinel-control/src/domain/recordings.rs`**

- Enrich each item with `camera_name` (from `devices` table by `camera_ip`) and `room_name`
  (from `map_markers` / presence config — nullable, empty string if not mapped).
- Forward all query params to the edge proxy call.

**UI**: replace the current in-memory list with a paginated server-driven table.

---

## Phase 3 — Camera + room columns (#5) + grouping (#6)

**Web UI additions**

- Table columns: `Name`, `Camera`, `Room`, `Date/Time`, `Duration`, `Size`, `Actions`.
- **Group by** dropdown: None / Camera / Room / Day. Groups are rendered as collapsible
  section headers; records inside are sorted by the active sort field.
- Group + sort are sent as query params; grouping is done client-side on the already-paginated
  page (a reasonable trade-off — grouping across pages requires a full-count query; defer that
  to v2).
- Camera filter + room filter chips (multi-select from available values).
- Date range picker (start/end date → `from` / `to` query params forwarded to edge).

**Edge**: add `from` / `to` RFC3339 params to `GET /recordings` for date-range filtering.

---

## Phase 4 — Real delete (#4)

**Edge route (new)**

```
DELETE /recording/:clip_id
  → 200 { ok: true } once MP4 + sidecar .json deleted from disk
  → 404 if not found
```

**Control**

- `DELETE /recordings/:id` → proxy DELETE to edge, then delete DB row on 200, return 200.
- If edge returns 404, still delete DB row (stale reference cleanup).

**Web UI**

- Delete button shows a confirmation dialog (shadcn `AlertDialog`), not `window.confirm`.
- On confirm: disable row, show spinner, `await api.deleteRecording(id)`, remove row on success.
- Error toast on failure; spinner clears.

---

## Phase 5 — Configurable max storage (#1)

**Settings UI (`sentinel-app/src/routes/settings/boxes/+page.svelte`)**

Add a "Recordings" section to the box detail panel:
- **Max storage (MB)** — number input (default 1000, min 100, max 100000).
- **Pre-roll seconds** — number input (default 5, min 2, max 30).
- **Codec** — select: `libx264` / `h264_v4l2m2m` / `h264_nvenc` / `h264_vaapi`.

**Control DB** (migration 0030 or 0031, after recordings table):
```sql
ALTER TABLE tenant_boxes ADD COLUMN recording_config JSONB NOT NULL DEFAULT '{}';
-- shape: { max_storage_mb, pre_roll_secs, codec }
```

**Control**: `PUT /settings/boxes/:id/recording-config` + `GET /config/recording` (edge pulls on boot).

**Edge**: on startup + config reload, read `max_storage_mb` from pulled config and pass to the
rotation eviction loop (`rotate_if_over_quota` in `recording.rs`).

---

## Sidecar .json per clip

Each finalized clip `foo.mp4` gets a `foo.json` alongside it:
```json
{
  "clip_id": "192_168_1_100_2026-06-23T14-32-00Z.mp4",
  "friendly_name": "swift-calm-ember",
  "camera_ip": "192.168.1.100",
  "duration_secs": 47,
  "size_bytes": 12345678,
  "started_at": "2026-06-23T14:32:00Z"
}
```
This lets `GET /recordings` reconstruct full metadata without a DB on the edge.

---

## Migration note

Next migration number after current state is **0030** (see CLAUDE.md). If the `recordings`
table doesn't exist yet (Phase 1 of original plan), create it here with `friendly_name` included.
If it exists from an earlier migration, add a column migration for `friendly_name` only.

---

## Out of scope (v1)

- Cross-page grouping totals (would need a COUNT(*) GROUP BY query — defer).
- Thumbnail generation for the list.
- S3/cloud upload.
- Multi-camera synchronized clips.
- Bulk delete.
