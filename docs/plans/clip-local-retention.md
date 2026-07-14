# Clip local retention — evict the local copy after cloud upload

**Board card:** #78 Clip local retention — evict local copy after cloud upload (ToDo)
**Goal:** Let the tenant choose to have the edge **delete a clip's local file once R2 confirms it's
durable**, turning the box's disk from a growing archive into a transient spool — so the appliance
we sell can ship with a small NVMe and no HDD at all.

Related: [[cloud-recording-storage]] (#64 — the R2 upload path this builds on),
[[storage-billing-economics]] (#71), [[edge-frame-hub]] (#77 — the CPU half of the same
appliance-sizing question), [[recordings-improvements]] (#22).

---

## Why

The edge already writes far less than people assume: the pre-roll ring is **in RAM**
(`recording.rs:178`, "no segments are written to disk between recordings"), clips are recorded only
on demand, and a finished clip is already PUT **straight to R2** from the box
(`clip_upload.rs`, `http.rs:210`). The one thing missing is the last step — `clip_upload.rs:9-11`
says it outright: *"the local copy is never deleted on our account"*. So the local disk still
accumulates until the 1 GB `max_storage_mb` evictor reclaims the oldest clips
(`recording.rs:505`).

Deleting on confirmed upload makes the local disk a pure spool. It also has to be **the tenant's
choice**, because for a tenant whose plan has no cloud recording it would mean deleting the only
copy that exists.

## What's true today (grounding)

- `sentinel-edge/src/http.rs:210` — on `POST /recording/stop`, the edge pushes the clip manifest to
  control; if the plan sells cloud recording, control answers with a presigned PUT, which is handed
  to `clip_upload::enqueue`.
- `sentinel-edge/src/clip_upload.rs:69-74` — the worker uploads, then POSTs
  `/config/recordings/{clip_id}/uploaded` to confirm. **This is the exact point the local file
  becomes redundant.**
- `RecordingConfig` (`recording.rs:102`) is the box's config blob — `tenant_boxes.recording_config`
  jsonb, edited from Settings (`sentinel-app/src/routes/settings/+page.svelte:80-90` →
  `PATCH /boxes/:id/recording-config`, `sentinel-control/src/domain/boxes.rs:32`).
  **It is fetched once at boot** (`recording.rs:219`) — see Open decisions.
- Playback is already cloud-first: `recordings/+page.svelte:195` `canPlay()` gates Play on
  `upload_state === "uploaded"` and `:203` streams from R2 via `cloudUrl`. The Playback timeline
  (`playback/+page.svelte:291`) tries `cloudUrl` and falls back to the edge proxy.
- **The one thing that breaks:** the Download button (`recordings/+page.svelte:149-153`)
  **always** goes through `/recordings/:id/download`, which proxies the file **from the edge**
  (`sentinel-control/src/domain/recordings.rs:217-250`). Evict the local file and Download 404s for
  a clip that is perfectly safe in R2. This must be fixed **in the same change**, not after.

## Phase 1 — the setting

- Add `delete_local_after_upload: bool` (default **false** — today's behaviour) to
  `RecordingConfig` (`sentinel-edge/src/recording.rs:102`, `#[serde(default)]`), and mirror it in
  `sentinel-app/src/lib/api.ts:120` `RecordingConfig` + `src/lib/types.ts:12`.
- No migration needed: `recording_config` is jsonb and every field is `#[serde(default)]`.

## Phase 2 — evict on confirmed upload (edge)

- Carry the flag on the upload job: add `delete_local: bool` to `clip_upload::Job`
  (`clip_upload.rs:18`), set at enqueue time in `http.rs:210` where the `RecordingManager` config is
  already in scope.
- In `clip_upload.rs`'s worker, **only after `confirm()` returns Ok** (`clip_upload.rs:69`), delete
  the clip's `<clip_id>.mp4` **and its `<clip_id>.json` sidecar**. Both — `StorageStats::compute`
  (`recording.rs:149`) and the evictor (`recording.rs:505`) read `size_bytes` from the sidecar, so
  leaving an orphan sidecar makes the box permanently over-report its disk usage.
- Order matters: upload → confirm → delete. A failed confirm must **keep** the file (control's
  sweep may mark the clip `failed`, and the local copy is then the only one left).
- Make the edge's `DELETE /recording/:clip_id` path idempotent — a clip-delete job (#64) for an
  already-evicted clip must succeed, not 404.

## Phase 3 — fix Download so it prefers the cloud (web)

`recordings/+page.svelte:149` `download(rec)`:

- When `rec.upload_state === "uploaded"`, fetch the presigned R2 URL (`recordings.cloudUrl`) and
  download from there — no VPS egress, works with the edge offline, works with the local file gone.
- Only fall back to the edge proxy (`recordings.download`) for a clip that is **not** in the cloud.
- Same rule already holds for `playback/+page.svelte:291`; verify its fallback branch can't be
  reached for an `uploaded` clip.

## Phase 4 — Settings UI

In the "Live feed and recording behaviour" card (`settings/+page.svelte:258`), next to
`cameras_always_warm` / `pre_roll_secs` / `max_storage_mb`:

- Switch: **"Delete local copy once saved to the cloud"**, described as *"The box keeps clips only
  until they're safely in the cloud. Frees the appliance's disk; clips still play and download from
  the cloud."*
- **Disable it, with an explanation, when the tenant's plan has no cloud recording**
  (`retention_days === 0` → nothing is ever uploaded → enabling it would be a no-op at best and a
  footgun at worst). The plan's cloud-recording fields are already on the billing plan
  (`api.ts:411`).
- Keep `max_storage_mb` visible and meaningful — with eviction on, it's the size of the **spool**
  (headroom for clips awaiting upload while the uplink is down), not of an archive. Reword its
  helper text accordingly.
- House style: no nested ternaries in the markup — named helpers returning bool/string.

## Verification

- Record a clip on a cloud-recording plan with the switch on → clip reaches R2 → the box's
  `recordings/` dir has neither the `.mp4` nor the `.json` → Play works (R2) → **Download works**
  (R2) → storage stats show the freed space.
- Kill the uplink, record 3 clips, restore it → all three upload, then evict. Nothing is lost while
  offline.
- Switch off (default) → behaviour is byte-for-byte what it is today.
- Delete a cloud clip whose local copy is already gone → the #64 clip-delete job completes clean.

## Open decisions (grill before building)

1. **`RecordingConfig` is only read at boot** (`recording.rs:219`; `boxes.rs:245` admits there's no
   live reload for it). Toggling this setting therefore needs an edge restart. Accept that for v1
   (the Boxes page has a Restart button, and the Settings copy can say so), or fold recording config
   into the existing 25s config poll? Folding it in is the honest fix and probably small — but it's
   a behaviour change for `pre_roll_secs` / `max_storage_mb` too.
2. **Keep a small local LRU cache instead of deleting immediately?** (e.g. keep the last N clips
   locally for instant playback while the browser is on the LAN). More code, and playback already
   goes to R2 — leaning **no**, delete immediately.
3. **Per-box or per-tenant?** Settings currently writes the same `recording_config` to **every**
   box (`settings/+page.svelte:80-90`). Keep that. Nothing here needs per-box divergence.
4. **Should this default to on for new tenants on a cloud plan?** It's the behaviour we actually
   want for the appliance we sell. Leaning: default off now, revisit once it has run in production.

## Out of scope (v1)

- Changing the pre-roll ring (already RAM-only — nothing to fix).
- Continuous 24/7 upload to R2 / cloud DVR. It's a different product: ~86 GB/day for 8 cameras,
  ~R$200/mo of R2 against a R$39.90 Home plan, and it blows through the 100 GB quota in
  [[storage-billing-economics]] on day one. If we ever sell it, it's a priced Business add-on,
  substream only — not a change to the default recording model.
- An authenticated browser-streamable proxy for **local** (never-uploaded) clips — still the
  standing gap noted at `recordings/+page.svelte:186-190`.
