# Faces — everything still missing, in order

**Board card:** #174 Faces — complete the pipeline
**Goal:** Close out face recognition. This card carries every remaining piece; the individual cards
it replaces (#17, #120, #128, #172) are folded in here.

## What already ships

Verified in production. Do not redo any of it.

| | |
|---|---|
| #117 | edge detect → embed → recognize → store |
| #122 | landmark-aligned ArcFace embeddings |
| #125 | L2-normalization asserted at store time |
| #119 | cluster + bulk-label workspace |
| #126 | quality signals — `blur`, `yaw`, `pitch`, `roll` as ports and columns |
| #127 | HNSW index + retrieve-then-rerank (2.35×→4.79×, recall 1.000) |
| #124 | clustering: SNN + coherence gate — one ambiguous face no longer welds two identities |
| #123 | per-identity scoring + margin test — implemented, MR !43 |

## Step 1 — Per-tenant face stream selector (was #120)

`sentinel-edge/src/frames.rs` **already** selects HD/unified for face capture. It is a **box-wide**
`face_capture_stream` setting. What is missing is making it **per tenant**.

- add the setting to the tenant config the edge pulls, alongside the existing per-box choice
- the edge already resolves main vs sub profiles; this is plumbing a preference, not new capture code

**Decision required, not implementation:** the original card also asked for **GPU decode**. That
contradicts [[face-compute-stays-cpu]], which records a benchmark showing GPU gave only ~2.4× and was
overhead-bound, with the recommendation to scale on a dedicated CPU box. **Recommend dropping the GPU
half explicitly** rather than leaving it to rot in a card nobody closes.

## Step 2 — Neighbours from the index (the rest of #124)

#124 fixed the *linkage rule*. It did not fix *scale*: `build_groups` still computes all pairs, which
is why `MAX_CLUSTER_N = 2000` exists — past that the workspace silently serves a flat list instead of
clusters.

- add `faces::neighbors(db, tenant_id, face_id, k)` served by the HNSW index
- feed the SNN graph from it instead of the O(n²) pass
- **delete `MAX_CLUSTER_N`** rather than raising it

The existing tests in `faces::clustering_tests` are the guard: the bridge case and the
"one identity still clusters" case must both keep passing when the neighbour source changes.

## Step 3 — Capture volume (the precondition for steps 4 and 5)

**This is the real blocker, and it is not code.** Production holds ~132 faces, and the quality
columns are populated only for captures since migration 0086 — fewer still.

Steps 4 and 5 are measurement work. Fitting thresholds on that sample means inventing numbers and
calling them measured, which is exactly what #126 refused to do.

Options, in preference order:
1. run a deliberate capture period on a real camera to build a sample worth fitting against
2. ship step 4 with a **stated provisional** default, labelled as such, and revisit
3. do not do steps 4 and 5 yet

## Step 4 — Default quality gate (was #172)

Once there is data:

- read the real distribution of `blur`, `face_width`, `confidence`, `yaw` off stored captures
- pick thresholds that keep what a human calls usable
- wire the gate into `seed_templates.rs::reconhece_rosto()` so new tenants get it by default
- document the defaults in `docs/workflow-nodes.md`

**Do not migrate existing tenants' graphs.** Replace-all graph persistence makes that a data-loss
shape ([[workflow-editor-explicit-save]]).

**Watch the old-edge case:** a box predating #126 reports no sharpness, and `run_num_cmp` resolves a
missing input to `0`, so a `NumGte(blur)` gate would fail closed and drop that tenant's whole
pipeline. Control already substitutes a fail-open sentinel; the default gate must not defeat it.

## Step 5 — Calibrated thresholds (was #128)

Replace the global constants with per-tenant/per-camera values fitted from data:

- `CLUSTER_EPS` (0.55), recognition `max_distance` (0.5), and the `margin` #123 left at 0
- cameras differ in sensor, lens and IR, so one number cannot be right for everyone
- subsumes step 4: a measured default is the degenerate case of a calibrated one

`margin` defaulting to 0 today is deliberate — a guessed value would start rejecting true matches.
This step is what earns a real one.

## Step 6 — Audit the old node card (was #17)

`face-recognition-next-steps.md` predates #117 shipping the entire pipeline, and proposes a GPU
sidecar upgrade that contradicts [[face-compute-stays-cpu]] the same way step 1 does.

**Read it against the shipped code before doing anything.** Most of it may already exist. Whatever
genuinely remains is probably small.

## Step 7 — Write the reference

`docs/ref/face-recognition.md`: one source-of-truth document — detection, quality signals, the gate,
embedding, retrieval and scoring, storage, clustering, and every constant in one table.

**Deliberately last.** Written earlier it documents an algorithm steps 1–6 are about to change, and a
stale source of truth is worse than none.

## Not in this card

**#59 ReID** — body-appearance matching, not faces. It shares only the "gallery + threshold" shape
and is a large independent track. Kept as its own card on purpose.

## Related
[[faces-roadmap]] · [[face-pipeline-hd-stream]] · [[face-clustering-chaining-fix]] ·
[[face-quality-gate-upstream]] · [[face-threshold-calibration]] · [[face-recognition-next-steps]] ·
[[face-compute-stays-cpu]] · [[face-knn-hnsw-rerank]]
