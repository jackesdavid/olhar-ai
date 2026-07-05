# Edge Resource Monitoring — Implementation Plan

**Board card:** #62 Edge resource monitoring — CPU / mem / disk / GPU (ToDo)
**Goal:** Ship the edge box's **CPU, memory, disk, and (if easy) GPU** usage to the cloud on the
existing heartbeat, store the latest snapshot on `tenant_boxes`, and surface per-box resource
gauges on the **Boxes** page — so an operator can confirm each edge is running optimally without
SSHing in.

## Architecture fit — reuse the health-reporting pipeline (don't invent a new one)

The edge already reports health on the ~25s config long-poll; resource metrics ride the same rails:

- **Edge → header build:** `with_health(req)` (`sentinel-edge/src/state.rs:422`) stamps
  `X-Edge-Tailscale` / `X-Edge-Ffmpeg` / `X-Edge-Platform`; `X-Edge-Version` in the `BoxReq` auth
  builder (`state.rs:452`). Headers **must be ASCII** (`state.rs:394` note).
- **Control → receiver:** `read_health_headers` (`sentinel-control/src/http.rs:93`) parses the
  `x-edge-*` headers; `touch_box_by_token` / `touch_box_identity`
  (`sentinel-control/src/store.rs:184` / `:218`) do a `COALESCE` UPDATE onto `tenant_boxes`,
  stamping `health_at` when health is present.
- **Storage:** `tenant_boxes` holds a **latest snapshot** (health columns added in
  `0018_box_health.sql`: `tailscale_state`, `ffmpeg_ok`, `health_at`; `platform` in `0034`). No
  time-series table today.
- **UI:** the Boxes page `sentinel-app/src/routes/settings/boxes/+page.svelte` renders per-box
  badges; box list is `GET /boxes` → `store::BoxRow` (`sentinel-control/src/domain/boxes.rs:53`,
  RBAC `devices.write`).

**Disk is already computed** (this is the "other feature" — reuse it):
- `StorageStats { used_bytes, max_bytes, pct, path }` (`sentinel-edge/src/recording.rs:140`).
- Eviction + alert loop every 60s (`sentinel-edge/src/main.rs:186`) → `push_disk_alert`
  (`state.rs:296`) → `POST /config/disk-alert` (`http.rs:304`) → `edge:full-disk` system event.
- On-demand live pull already wired: `GET /boxes/:id/storage-stats` (`boxes.rs:34`).
- So disk usage is available now — the monitor ships it **continuously** (not only at the 90%
  alert), alongside CPU/mem/GPU.

Edge deps today (`sentinel-edge/Cargo.toml`): `ureq` (json), `serde`, `opencv` — **no `sysinfo`**.
The edge is **not** in the cargo workspace (own manifest, links native OpenCV), so adding a dep here
never touches the cloud image build.

## Phase 1 — Edge: sample the metrics

- Add **`sysinfo`** to `sentinel-edge/Cargo.toml` — cross-platform (Linux + macOS) CPU% and memory
  used/total; also filesystem disk. Keep one long-lived `System` refreshed on a cadence (CPU% needs
  two samples ~200ms apart to be meaningful).
- Build an `EdgeMetrics` struct (new `sentinel-edge/src/metrics.rs`):
  - `cpu_pct` (system-wide, 0..100), optional `load_avg` + `cores`.
  - `mem_used_mb`, `mem_total_mb`, `mem_pct`.
  - `disk`: **reuse `StorageStats`** for the clip-storage gauge (used vs configured max), and
    additionally `sysinfo` filesystem free/total for the data partition (see decision #3).
  - `gpu` (optional): shell out to `nvidia-smi --query-gpu=utilization.gpu,memory.used,memory.total
    --format=csv,noheader,nounits`; parse → `gpu_pct`, `gpu_mem_used_mb`, `gpu_mem_total_mb`.
    Absent on macOS / non-NVIDIA → `None` (guarded so a missing `nvidia-smi` never errors the loop).
- Sampling cadence: refresh a cached `EdgeMetrics` every ~10–15s in a small thread (or fold into the
  existing 60s eviction loop / motion manager) so the heartbeat just reads the latest cached value.

## Phase 2 — Transport: piggyback the heartbeat

- **Recommended (v1):** add a single compact JSON header **`X-Edge-Metrics`** in `with_health`
  (`state.rs:422`) — e.g. `{"cpu":37,"mem":54,"disk":61,"gpu":22}` (+ raw MB fields). JSON is
  ASCII-safe, and it rides the existing ~25s config poll → **zero new request or loop**, exactly
  like the health headers.
- **Alternative:** a dedicated `POST /config/metrics` (mirrors `/config/motion`, `/config/logs`,
  `/config/disk-alert`) if we later want a faster cadence or to feed a history table without coupling
  to the 25s poll. Note it as the upgrade path; don't build it for v1.

## Phase 3 — Control: parse + store the snapshot

- Migration `00NN_box_metrics.sql` (next free number — 0038 unless the ReID plan's 0038 lands first,
  then 0039): add snapshot columns to `tenant_boxes`, mirroring the `0018` health columns:
  `cpu_pct real`, `mem_pct real`, `mem_used_mb integer`, `disk_pct real`, `gpu_pct real` (null),
  `gpu_mem_pct real` (null), `metrics_at text` (RFC3339 — sqlx can't decode `timestamptz`; note the
  health columns predate that rule and use `timestamptz`, so keep `metrics_at` as **text** to stay
  decode-safe, or follow `health_at`'s `timestamptz` if it's never selected into a typed struct).
- Extend `box_health_headers` (`http.rs:96`) to parse `X-Edge-Metrics`, and update **both**
  near-duplicate UPDATEs — `touch_box_by_token` (`store.rs:184`) *and* `touch_box_identity`
  (`store.rs:218`) — to `COALESCE` the new columns + set `metrics_at = now()` when metrics are
  present (same shape as `health_at`). (Missing one silently drops metrics on that path.)
- Add the fields to `store::BoxRow` (`store.rs:122`) **and** the `list_boxes` SELECT
  (`store.rs:163`, currently selects the health columns) so `GET /boxes` (`boxes.rs:53`, RBAC
  `devices.write`) returns them — no new route needed.

## Phase 4 — UI: resource gauges on the Boxes page

- `settings/boxes/+page.svelte`: the **Health cell** (lines ~249–265, alongside the `tsClass()`/
  `tsLabel()` badges) is where compact **CPU / Memory / Disk** bar gauges go per box (+ **GPU** only
  when `gpu_pct` is non-null). Monochrome ink theme + `.status-live` — no brand hue; convey level by
  fill, and switch to the existing **warning** treatment (the offline / ffmpeg-missing badge style)
  when a metric is sustained-high (≥ ~85%). Show `metrics_at` freshness (stale if older than a
  couple of heartbeats).
- The page uses **`$lib/api`** (`import { boxes as boxApi, type Box }` — NOT `lib/data.ts`); add the
  new numeric fields to the **`Box` type in `sentinel-app/src/lib/api.ts:65`** so `boxApi.list()`
  (`api.get<Box[]>("/boxes")`) type-checks. No new API method.
- Keep markup logic out of the template (named helpers returning the gauge state / warn bool), per
  house style.

## Phase 5 — "Is it optimal?" alerting (optional, defer)

Reuse the `edge:full-disk` pattern for pressure signals: emit an `edge:high-load` system event
(`repo::system_events()` catalog, like `edge:full-disk` in `workflows/repo.rs:304`) when CPU or mem
is sustained-high, pushed from the edge like `push_disk_alert` — so tenant workflows can notify on a
struggling box. Ship the gauges first; add alerting once thresholds are calibrated from real data.

## Open decisions (grill before building)

1. **Transport.** Heartbeat `X-Edge-Metrics` header (v1, zero new request) vs. a dedicated
   `POST /config/metrics` (needed only for faster cadence / history). Recommend **header for v1**.
2. **Snapshot vs. history.** Latest snapshot on `tenant_boxes` (simple, matches health) vs. a
   `box_metrics` time-series table (like `box_logs`) for **sparklines / trends** + retention pruning.
   Recommend **snapshot v1**, history as a fast follow if the UI wants trend lines.
3. **Disk semantics.** Show the existing **clip-storage %** (`StorageStats`, used vs configured max)
   or the **filesystem %** (whole data partition via `sysinfo`), or both. Recommend **both** —
   "Recordings" gauge (reuses existing stats) + "Disk" gauge (filesystem), since a full OS disk and a
   full clip quota are different failure modes.
4. **GPU scope.** "If easy" = **NVIDIA-only via `nvidia-smi`**; macOS/AMD/Intel report null (gauge
   hidden). Confirm that's acceptable, or drop GPU from v1 entirely.
5. **Cadence.** Piggyback the ~25s heartbeat (fine for a health monitor) vs. faster (requires the
   POST). Recommend 25s.
6. **Warn thresholds + where they live.** Hard-coded (~85%) for v1, or per-tenant/per-box configurable
   (JSONB like `tracking_config`)? Recommend hard-coded v1, configurable later with Phase 5 alerting.

## Out of scope (v1)

- Per-process attribution (which process is eating CPU/mem).
- Long-horizon historical charts (a short sparkline at most, only if the history table lands).
- Network throughput / bandwidth and temperature/thermal metrics.
- Non-NVIDIA GPU utilization.
- Auto-remediation (restarting/throttling a struggling box) — monitoring + alerting only.
