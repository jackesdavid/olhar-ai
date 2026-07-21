# Per-Tenant / Per-Camera Similarity Threshold Calibration — Implementation Plan

**Board card:** Similarity thresholds — per-tenant/camera calibration (ToDo)
**Goal:** Replace the global magic constants (clustering ε = 0.55, recognition gate 0.40) with
data-driven thresholds measured per tenant/camera. Cameras vary (sensor, lens, IR), so one number
can't be right for everyone.

## Why
`CLUSTER_EPS = 0.55` (`faces.rs:236`) and the recognition threshold are hand-tuned global constants,
justified once on a single fixture ([[faces-clustering-and-proxy]]). Different cameras produce
different intra/inter distance distributions; the right cut is where those distributions cross.

## Phases
1. **Measure distributions.** Once any labeled data exists, compute intra-person (same person_id) vs
   inter-person nearest-neighbour distance histograms per tenant, and per camera where volume allows.
   Reuse the recognition kNN path ([[face-knn-hnsw-rerank]]).
2. **Pick the threshold** at the crossover / max-F1 point per scope; fall back to the global default
   when a tenant/camera lacks enough labeled samples.
3. **Store calibrated values** (per-tenant, optionally per-camera) — small settings row, applied as
   the *default* ε and recognition gate. The live `?eps=` slider stays as manual override on both
   views.
4. **Recalibrate** on a schedule / after bulk labeling (pg_cron job, or a "recalibrate" action).
5. **Re-baseline after aligned embeddings land** ([[face-embed-landmark-align]]) — alignment shifts
   the distributions and should lower the defaults.

## Open decisions
1. Per-camera calibration from day one, or per-tenant first and per-camera once volume justifies it.
2. Minimum labeled-sample count before trusting a calibrated value over the global default.

## Out of scope (v1)
- Online/continuous learning. Batch recalibration only.

## Related
[[faces-clustering-and-proxy]] · [[face-recognition-per-person-scoring]] · [[face-knn-hnsw-rerank]] · [[face-embed-landmark-align]]
