# Face Quality Gate (Blur + Pose) Upstream — Implementation Plan

**Board card:** #126 Face quality gate — blur + pose before embed
**Goal:** Stop low-quality faces (tiny, blurry, extreme-pose) from ever being embedded, so they
never pollute the gallery, the clusters, or the recognition thresholds. Composed as a **graph
pattern**, not baked into any node.

## Why

Everything downstream is garbage-in/garbage-out — `RecognizeFace` margins ([[face-recognition-per-person-scoring]]),
clustering ε ([[face-clustering-chaining-fix]]), threshold calibration ([[face-threshold-calibration]]).
Today the only quality signals that reach the graph are `confidence` and `face_width`. A blurry,
90°-yaw, 40-pixel face embeds into a vector that sits in the middle of nowhere and drags clusters
with it.

Filtering must live **upstream in the workflow**, because `StoreFace` is a pure sink and must never
decide which faces to store ([[storeface-pure-sink]]). The gate is therefore composed from ports —
which means every quality signal has to *be* a port.

## What exists today (verified)

| Piece | Where |
|---|---|
| Detection + event push | `sentinel-edge/src/face.rs::detect_and_push` — has `bgr`, `(fw,fh)`, `face.xyxy`, `face.kps` in hand |
| Wire format | `sentinel-edge/src/state.rs::FaceEvent` (~:557) — `bbox`, `confidence`, `face_width`, `kps`, `at`, `frame_b64` |
| Ingest | `sentinel-control/src/http.rs::face_push` (~:900) — `FaceIn` → payload → `enqueue_event_blocking(…, "face_detected", "edge", …)` |
| Port schema | `crates/app-core/src/workflows/repo.rs::system_events()` (~:485) — `camera`, `bbox`, `confidence`, `face_width`, `kps`, `frame` |
| Detector | `crates/app-core/src/agent/face_detector.rs` — `FaceBox { xyxy, score, kps: [[f32;2];5] }` (right eye, left eye, nose, right mouth, left mouth) |
| Sink | `crates/app-core/src/workflows/node/vision.rs::run_store_face` (:323) — persists `confidence` + `face_width` via input ports |
| Storage | `faces` table; `face_width` replaced `face_ratio` in migration `0075_face_width.sql` |
| Gate nodes | `NumGte`, `NumLte`, `And`, `If` (→ "Branch") all already in `NodeKind::ALL` |
| Shipped default graph | `sentinel-control/src/seed_templates.rs::reconhece_rosto()` (~:451) — currently has **no gate at all** |

**Correction to the earlier sketch:** it referred to `face_ratio`. That signal is gone — `0075`
dropped the column and replaced it with `face_width` (box width in source pixels), because a ratio
riding `PortType::Num` lost its precision. Gate on `face_width`.

## Phases

### Phase 1 — Blur metric at the edge
`detect_and_push` already holds the decoded BGR buffer and the face box, so sharpness costs one
pass over a few thousand pixels and **no re-decode, no extra model**.

- Add `fn sharpness(bgr, (fw,fh), xyxy) -> f32` in `sentinel-edge/src/face.rs`: crop to the face
  box, convert to luma, 3×3 Laplacian, return the **variance**.
- Normalize so the number is comparable across face sizes — variance-of-Laplacian scales with
  resolution, so a big blurry face can out-score a small sharp one. Divide by face area (or compute
  on a fixed-size resample of the crop, e.g. 64×64) and pick one; document which.
- Add `blur: f64` to `FaceEvent`.

### Phase 2 — Pose metric from the 5 landmarks
Pure arithmetic on `face.kps` — no model, no sidecar. Shares the landmark plumbing already landed in
[[face-embed-landmark-align]].

- `yaw` — horizontal asymmetry of the nose between the two eye centres: negative = turned one way,
  positive = the other. Normalize by inter-ocular distance so it is scale-free.
- `roll` — angle of the eye-to-eye line off horizontal (`atan2`), exact and free.
- `pitch` — nose height relative to the eye line vs the mouth line, normalized the same way.
  Weakest of the three from 5 points; ship it, but expect the useful gate to be `yaw` + `roll`.
- Emit all three as degrees (or a documented normalized unit) on `FaceEvent`.

### Phase 3 — Carry the signals through to the graph
1. `sentinel-control/src/http.rs` — add `blur`, `yaw`, `pitch`, `roll` to `FaceIn` (all `#[serde(default)]`,
   so an older edge keeps working) and into the `face_detected` payload JSON.
2. `crates/app-core/src/workflows/repo.rs::system_events()` — add four `PortType::Num` fields to the
   `face_detected` entry. **This is a code catalog, not a migration** (CLAUDE.md). Adding them here
   is what makes them output ports on every tenant's `When Event` node, with no per-tenant
   registration.
3. `PortType::Num` is integer-valued on the wire. Blur and pose are fractional — either scale to a
   documented integer unit (degrees for pose; blur ×100) or introduce a float port type. **Scale to
   integers** — it matches what `confidence` (0..100) already does and avoids a port-type change
   rippling through the editor. Note the unit in the event description string.

### Phase 4 — Persist the signals on the stored face
So the Faces workspace can *show* why a face was kept and calibration ([[face-threshold-calibration]])
has real data to fit against.

- One migration adding `blur`, `yaw`, `pitch`, `roll` to `faces` (`real NOT NULL DEFAULT 0`).
  **`ls crates/app-core/migrations/` for the next number** — `0084` is the highest today.
- `crates/app-core/src/faces.rs` — add the fields to `NewFace` + the `insert` statement, and to
  `Face`/`FaceDetail` selects. Every `.bind()` added must match a new `$n`: this is exactly the
  class of drift that took box auth down, and `check:sql` (`crates/app-core/tests/sql_prepare.rs`)
  only catches it if the statement stays literal SQL.
- `run_store_face` reads them from input ports like it already does `confidence`/`face_width`.
  **It still makes no decision** — it stores what it is handed ([[storeface-pure-sink]]).

### Phase 5 — Ship the default gate
The gate is now composable with existing nodes:

```
When Event(face_detected)
  ├─ face_width  → NumGte(min_width) ─┐
  ├─ confidence  → NumGte(min_conf)  ─┤
  ├─ blur        → NumGte(min_blur)  ─┼→ And → Branch(true) → Extract Face Embedding → …
  └─ |yaw|       → NumLte(max_yaw)   ─┘
```

- Update `seed_templates.rs::reconhece_rosto()` so new tenants get the gate wired by default. This
  is a code change (the template is seeded from `seed_templates.rs`), *not* a UI graph edit.
- Existing tenants' graphs are untouched — document the pattern so they can add it. Do **not**
  migrate live tenant graphs; replace-all graph persistence makes that a data-loss shape
  ([[workflow-editor-explicit-save]]).

### Phase 6 — Document the defaults
Add the ports + recommended starting thresholds to `docs/workflow-nodes.md` (the `face_detected`
event section) and to [[face-pipeline-architecture]].

## Open decisions

1. **Blur normalization** — divide-by-area vs fixed-size resample. Resample is more honest across
   camera resolutions; area-divide is cheaper. Measure on real captures before choosing.
2. **Starting thresholds** — must be measured, not guessed. Pull a few hundred real `faces` rows,
   compute the metrics offline, and pick thresholds that keep the faces a human calls usable.
   Per-camera calibration is [[face-threshold-calibration]], not this card.
3. **Integer scaling unit for `blur`** — its raw range depends on the normalization chosen in (1);
   fix the unit only after (1) is decided.
4. **Old edges** — boxes that have not updated send no quality fields, so they default to 0 and a
   `NumGte(min_blur)` gate rejects **everything** from them. Either default missing quality fields
   to a pass-through sentinel, or gate the gate on edge version. Decide before shipping Phase 5.

## Out of scope (v1)

- A dedicated "FaceQuality" node — keep it composed from existing comparison nodes.
- Per-camera thresholds ([[face-threshold-calibration]]).
- Re-scoring the faces already stored (a backfill would need the original frames; the crops are
  cropped and the pose is unrecoverable from a crop alone).

## Related
[[storeface-pure-sink]] · [[faces-workspace]] · [[face-embed-landmark-align]] ·
[[face-pipeline-architecture]] · [[face-knn-hnsw-rerank]] · [[face-threshold-calibration]]

---

## Outcome (shipped 2026-08-03, commit `ad08d32`)

Metrics live in `crates/app-core/src/agent/face_quality.rs` (shared by edge + cloud), computed on
the edge inside the existing detect loop. Ports registered on `face_detected`; `Store Face` records
them via migration `0086_face_quality.sql`, columns NULLABLE so "not measured" stays distinct from
"measured as zero".

**Open decision 1 (blur normalization) → resolved as fixed-size resample**, to the embedder's
112×112 input rather than divide-by-area: what matters is the detail surviving into the vector, and
it makes thresholds transfer across cameras of different resolutions (asserted by a test).
**Open decision 3 (integer unit) → no scaling needed**; raw sharpness is already in the hundreds to
tens of thousands, so it rides `PortType::Num` directly.

**Open decision 4 (old edges) → confirmed as a real trap, then fixed.** `run_num_cmp` resolves a
missing input to `0`, so a `NumGte(blur)` gate would have failed CLOSED and silently dropped the
entire face pipeline of any box predating this card. Control substitutes
`face_quality::BLUR_UNKNOWN`, an explicit fail-open sentinel, which `StoreFace` maps back to NULL so
"unknown" never enters calibration data as "excellent".

11 unit tests prove the metrics rather than exercise them: sharpness falls monotonically under
successive blur (49218 → 52 over 8 passes), is resolution-invariant, and is ~0 on a flat patch; roll
is recovered exactly; yaw survives ±25% anthropometric variation and stays monotonic and correctly
signed. The pose tests generate landmarks from a 3-D model with deliberately perturbed proportions,
so they measure robustness rather than replaying the estimator's own algebra.

**Still open — Phase 5 was deliberately NOT done (carved out as card #172).** The gate is not wired into the seeded template
and no default thresholds ship. Thresholds must be measured off real captures, and inventing them
would be worse than shipping none. That work belongs with [[face-threshold-calibration]], which now
has the columns to fit against.
