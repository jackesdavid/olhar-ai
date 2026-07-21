# Assert L2-Normalization at Embedding Store Time — Implementation Plan

**Board card:** Assert L2-normalization on face embeddings at store time (ToDo)
**Goal:** Guarantee every stored ArcFace vector is unit-norm so the DB path and the in-Rust clustering
path compute the *same* cosine distance. Cheap correctness guardrail.

## Why
Two distance code paths make different assumptions:
- **Recognition** uses pgvector `<=>` (`faces.rs:107`, `:143`), which normalizes internally — safe
  even if a vector isn't unit-norm.
- **Clustering** uses `cosine_distance = 1 - dot` in Rust (`faces.rs:241`). This equals cosine
  **only if both vectors are L2-normalized.** If the sidecar ever returns an un-normalized embedding,
  clustering distances go quietly wrong while recognition still looks fine — a silent, hard-to-spot
  quality regression.

## Phases
1. **Normalize on insert.** In the StoreFace write path (`faces.rs` insert + person-embedding
   enrollment), L2-normalize the vector before it hits `vector(512)`. Idempotent for already-normal
   vectors.
2. **Debug assert.** Add a `debug_assert!((norm - 1.0).abs() < 1e-3)` (or a warn-log in release) at
   store time to catch sidecar contract drift early.
3. **Confirm sidecar output.** Verify `ai/sidecar` buffalo_l returns normalized embeddings for all
   three ops (`face_detect_embed`, `face_embed`, `embed`); if not, normalize server-side too.
4. **Backfill check.** One-off query over existing `faces` / `person_embeddings` to confirm norms are
   ~1.0; renormalize any strays.

## Out of scope (v1)
- Switching clustering to call pgvector for distances (keeps the fast in-memory path; this card just
  makes its assumption safe).

## Related
[[faces-clustering-and-proxy]] · [[face-pipeline-architecture]] · [[face-embed-landmark-align]]
