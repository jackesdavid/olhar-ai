# Edge detection timing metrics — motion + face detect ms in the resource table

**Board card:** #118 Edge detection timing metrics — motion/face detect ms + rate (ToDo)
**Goal:** Measure how long (and how often) the edge's detectors run — **motion (MOG2)** and **face
(YuNet)** — and surface the rolling average on the Boxes **Resources** table, alongside CPU/MEM/DISK.
So an operator sees "motion 3 ms · face 41 ms · N faces/min" per box and can spot an overloaded appliance.

## Why (owner, 2026-07-18)
Detection cadence today is *config-governed but never measured*: motion runs per hub frame (≈`fps`/s,
`frames.rs`), face runs at most once per `min_interval` on a qualifying motion emit. There's no
timing or rate recorded — you can't tune what you can't see.

## Grounding (reuse the existing metrics channel — no new transport)
- **Edge** already samples `EdgeMetrics { cpu, mem, disk, gpu, gpu_mem }` (`crates/app-core/src/edge_metrics.rs`)
  and ships it on the heartbeat via the compact `X-Edge-Metrics` header (`sentinel-edge/src/metrics.rs`,
  a timer-refreshed cache read by `state::with_health`).
- **Control** parses the header → upserts into `tenant_boxes` (`store.rs` ~L138/223; migration
  `0038_box_resource_usage.sql` added `cpu_pct`/…/`metrics_at`).
- **Frontend** renders the **Resources** column from those columns (`settings/boxes/+page.svelte`
  L164-171, table header L271).
- The detectors to time: MOG2 (`sentinel-edge/src/mog2.rs`, run in `frames.rs`), YuNet
  (`app_core::agent::face_detector::detect[_bgr]`, run in `sentinel-edge/src/face.rs`).

## Phases
1. **Edge measurement.** Wrap each MOG2 motion detect + each YuNet face detect with `Instant::now()`;
   keep a **rolling average (EWMA)** per box in the metrics cache (a `Mutex<Timings>` or atomics), plus
   a **count** for a rate. Cheap; no per-detect allocation.
2. **`EdgeMetrics` fields.** Add `motion_ms: Option<u16>`, `face_ms: Option<u16>` (avg), and optionally
   `face_per_min: Option<u16>`. Serde `skip_serializing_if=None` (back-compat header). Shipped in the
   existing `X-Edge-Metrics` — no new endpoint.
3. **Control store.** Migration **0062**: `tenant_boxes` + `motion_detect_ms`/`face_detect_ms`
   (+ `face_per_min`) integer columns. Parse from the header + `COALESCE` upsert (mirror `cpu_pct`).
   **Validate the migration on the live DB before deploy** ([[validate-migrations-live-db]]).
4. **Frontend.** Show avg **motion / face detect ms** (+ rate) in the Resources column (or a hover/detail),
   matching the CPU/MEM/DISK style. Null when a box hasn't reported (older edge).

## Open decisions
1. **Averaging window** — EWMA (α≈0.2) vs a fixed N-sample mean. Lean EWMA (O(1), no buffer).
2. **Per-camera vs per-box** — v1 a single box-level average (all cameras folded); per-camera is a
   later drill-down.
3. **Rate metric** — include faces/min (needs a count + window) or ship durations only in v1.

## Out of scope (v1)
- Per-camera / per-node timing history + charts; a time-series store.
- Timing the cloud pipeline (embed/recognize/store) — that's the instance view / run logs' job.
