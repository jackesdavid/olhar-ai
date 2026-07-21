# Landmark-Aligned Face Embeddings — Implementation Plan

**Board card:** Face embeddings — landmark-aligned (kps → face_embed) (ToDo)
**Goal:** Feed ArcFace *aligned* crops using the edge's existing YuNet 5-point landmarks, instead of
falling back to the align-free `face_embed`. Aligned embeddings tighten every downstream distance
(recognition margin, cluster purity, dedup) at once — the single highest-ROI quality lever.

## Reconnaissance findings (2026-07-20 — grounds the steps below)
- **The edge already computes `kps` and discards it.** `crates/app-core/src/agent/face_detector.rs`
  runs YuNet with 12 outputs incl. `kps` (g·g·10 = 5 pts × xy) at output index **`9+si`**, but
  `run_and_decode` only reads cls/obj/bbox and `FaceBox` keeps just `xyxy`+`score`. So this is
  *surface it*, not *compute it* — decode `kps` (linear, like the bbox centre: `(grid+Δ)·stride/ratio`)
  and carry it on `FaceBox` (stays `Copy`; `[[f32;2];5]`).
- **Non-regressive rollout.** `run_face_embedding` (`workflows/node/vision.rs:165`) crops to bbox and
  calls `embed_face_crop`, which already falls back to align-free `face_embed` when kps/SCRFD miss. So
  the app-core plumbing can land first; **absent kps → existing fallback → zero behavior change** until
  the edge emits kps AND the sidecar aligns from it.
- **Coordinate translation is the fiddly bit.** The edge emits kps in SOURCE-frame pixels;
  `run_face_embedding` crops to bbox+30% margin (`crop_to_bbox`, vision.rs:196). kps must be
  translated into the crop's local coords (subtract crop origin) before the sidecar aligns — so
  `crop_to_bbox` must also return the crop offset.
- **Port + type:** `PortType`/`PortValue` live in `workflows/domain.rs` (Bbox at :56-63, Embedding at
  :58 — the pattern to copy). `face_detected` is registered in `workflows/repo.rs:461 system_events()`
  (entry :483). Sidecar bridge is `agent/face.rs` — `face_embed` op currently takes only `image_b64`.

## Slices (revised, in build order)
- **A — app-core plumbing (fully cargo-checkable, non-regressive):** decode+carry kps on `FaceBox`;
  add `PortType::Landmarks` / `PortValue::Landmarks`; register `kps` on `face_detected` + expose as a
  When Event output port; add `embed_face_crop_aligned(kps)` in `agent/face.rs`; thread kps +
  crop-offset through `run_face_embedding`. Dormant (fallback) until B+C. **← current slice**
- **B — edge emit:** `FaceEvent.kps` (`sentinel-edge/src/state.rs`) + emit in `sentinel-edge/src/face.rs`
  + control ingest. Needs the out-of-workspace edge build (OpenCV) + an edge binary re-release.
- **C — sidecar align:** `ai/sidecar` `face_embed` accepts `kps` → InsightFace `norm_crop` from the
  provided landmarks (buffalo_l), skipping SCRFD. Only when B+C land does the quality win activate.

## Why
Today `Extract Face Embedding` (`crates/app-core/src/workflows/node/vision.rs::run_face_embedding`)
calls `agent::face::embed_face_crop`, which tries the sidecar's `face_detect_embed` (SCRFD re-detect +
align + ArcFace) and, when SCRFD can't re-find the face in the crop, falls back to align-free
`face_embed`. Align-free ArcFace vectors are noisier — that noise is *why* clustering ε had to be
loosened to 0.55 and intra-person spread measures 0.50–0.56 ([[faces-clustering-and-proxy]],
[[face-compute-stays-cpu]]). The edge's YuNet already computes 5 keypoints per face; we just don't
carry them to embed time.

## Phases
1. **Emit `kps` from the edge — as SHARED, reusable plumbing.** `sentinel-edge/src/face.rs` — add the
   YuNet 5 landmark points to the `face_detected` System event payload (alongside `bbox`, `confidence`,
   `face_ratio`, `frame`), registered in `repo::system_events()`. Give landmarks a **proper typed
   `PortType`/`PortValue`** (a real landmark type, NOT a bare NumArray — [[workflow-bbox-typed]]
   discipline). **This is the one source of `kps` for the whole face pipeline** — [[face-quality-gate-upstream]]
   (#126) consumes the exact same port for pose (yaw/pitch), and any future node reuses it instead of
   re-detecting. Whichever of #122 / #126 lands first owns this emission + port; the other just
   consumes it. Do NOT invent a second landmark path. Keep [[keep-agent-tools-in-sync]] in mind.
2. **Expose `kps` on `When Event("face_detected")`** as a new typed output port (the shared one from
   Phase 1) — consumed here for embedding alignment and by #126 for pose.
3. **Thread kps into embedding.** `run_face_embedding` passes `kps` to `agent::face::face_embed` so
   the sidecar aligns with the provided landmarks (`face_embed(kps)` path) and **skips SCRFD
   re-detection** — accurate AND ~460 ms → ~65–100 ms on the CPU we already run
   ([[face-compute-stays-cpu]]).
4. **Sidecar contract.** `ai/sidecar` — confirm/extend `face_embed` to accept 5 keypoints and do the
   InsightFace norm-crop alignment from them (buffalo_l already returns kps in `face_detect_embed`).
5. **Fallback stays.** If kps absent (old events, non-edge sources), keep the current
   `face_detect_embed` → `face_embed` fallback chain so no face is ever dropped.
6. **Re-tighten ε.** After aligned vectors flow, re-measure intra/inter distance and lower
   `CLUSTER_EPS` (`faces.rs:236`) + the recognition threshold accordingly (coordinate with
   [[face-threshold-calibration]]).

## Out of scope (v1)
- Re-embedding the existing ~128 stored faces (they lack kps); new captures get aligned vectors,
  a backfill can come later if needed.

## Related
[[face-pipeline-architecture]] · [[faces-workspace]] · [[face-compute-stays-cpu]] · [[workflow-bbox-typed]]
