# Face Quality Gate (Blur + Pose) Upstream — Implementation Plan

**Board card:** Face quality gate — blur + pose before embed (ToDo)
**Goal:** Stop low-quality faces (tiny, blurry, extreme-pose) from ever being embedded and polluting
the gallery + clusters. Composed as a *graph* pattern, not baked into any node.

## Why
Everything downstream is garbage-in/garbage-out. Today only `confidence` and `face_ratio` are captured
(`face_detected` event) — a real gate also needs **blur** and **pose**. Filtering must live upstream in
the workflow (`When Event → NumGte → Branch → Extract Face Embedding`), because StoreFace is a pure
sink and must never decide which faces to store ([[storeface-pure-sink]]).

## Phases
1. **Blur metric.** Add variance-of-Laplacian (sharpness) on the face crop — cheapest at the edge
   (`sentinel-edge/src/face.rs`) or in `run_face_embedding` pre-embed. Emit as a numeric field.
2. **Pose metric.** Derive yaw/pitch from the 5 landmarks (shares the kps work in
   [[face-embed-landmark-align]]) — emit as numeric fields.
3. **Expose EVERY quality variable as an output port on `When Event("face_detected")`** — a gate can
   only be composed from signals the graph can see, so all of these become first-class ports (and are
   registered in `repo::system_events()`, [[keep-agent-tools-in-sync]]):
   - `confidence` — detector score 0..100 (already emitted)
   - `face_ratio` — face_area ÷ frame_area × 100 (already emitted)
   - `blur` — variance-of-Laplacian sharpness (new, Phase 1)
   - `yaw`, `pitch` (and `roll` if cheap) — head pose from the 5 landmarks (new, Phase 2)
   - `kps` — the raw landmark points (shared with [[face-embed-landmark-align]]), so downstream nodes
     can reuse them without re-detecting
   Each port typed properly (numeric scalars; kps as the landmark port type) — no bare NumArray for
   things that have a real type ([[workflow-bbox-typed]] discipline). StoreFace still *persists* these
   values via its input ports (storing, not deciding — [[storeface-pure-sink]]).
4. **Ship the gate recipe.** The graph pattern (extends the 4-node one prepared in [[faces-workspace]]):
   `When Event → NumGte(confidence) + NumGte(face_ratio) + NumGte(blur) + pose-in-range(yaw/pitch) →
   And → Branch → Extract Face Embedding`. Because every signal is now a port, the tenant composes any
   gate they want. Graph edit is a UI action (replace-all persistence + prod-DB write is
   safety-blocked), not code.
5. **Document defaults** so tenants get a sane starting gate.

## Open decisions
1. Blur/pose thresholds — calibrate per camera ([[face-threshold-calibration]]).
2. Compute blur/pose at the edge (saves a round-trip, adds edge CPU) vs in the flow node.

## Out of scope (v1)
- A dedicated "FaceQuality" node — keep it composed from existing comparison nodes for now.

## Related
[[storeface-pure-sink]] · [[faces-workspace]] · [[face-embed-landmark-align]] · [[face-pipeline-architecture]]
