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

## Step 2 — Neighbours from the index (the rest of #124) — **DONE**

#124 fixed the *linkage rule*. It did not fix *scale*: `build_groups` computed all pairs, which is
why `MAX_CLUSTER_N = 2000` existed — past that the workspace silently served a flat list instead of
clusters.

Shipped: `faces::knn_neighbours` sources the SNN edge list from one `CROSS JOIN LATERAL` top-k probe
per face, plan-verified as `Index Scan using faces_embedding_hnsw`.

### Two things the plan got wrong, corrected by measurement

**"Delete `MAX_CLUSTER_N` rather than raising it."** It was raised to 20 000, not deleted, and the
reason changed. The cap no longer bounds CPU — it bounds **memory**, because the coherence gate and
the cluster centroids still hold every embedding in Rust at ~2 KB each.

That was challenged directly ("por que mesmo precisamos dos embeddings em memória?") and the answer
was tested rather than asserted. Coherence *can* be computed entirely in SQL — `avg(vector)` exists,
and a self-join on the edge list works — but measured **5959 ms for 1000 faces** at its best
(embeddings carried inline in the CTE to avoid re-fetching), against roughly 5 ms in Rust. The cost
is not the arithmetic; it is a hash join emitting 45k rows each carrying two 2 KB vectors. So
in-memory coherence is a **performance** decision, not a necessity, and the cap is the price.

**The existing unit tests were not sufficient as the guard.** They pass whatever the neighbour source
is, because they test the linkage rule on an in-memory graph. Two real defects got through them and
were caught only by a DB-backed test against seeded ground truth
(`clustering_recovers_seeded_identities_via_the_index`):

1. **`LIMIT KNN_K` inside the LATERAL is the wrong semantics.** `knn_within` means "the k nearest
   that are ALSO within eps" — filter, then truncate. `LIMIT k` truncates first, so the eps filter
   leaves fewer than k neighbours, overlap drops, `MIN_SHARED` fails, and identities fragment. On a
   200×25 gallery: 1588 clusters, mean 17.9 of 25 recovered, 53 identities split. Over-fetching
   `KNN_CANDIDATES = KNN_K * 4` and truncating in Rust: 232 clusters, mean 24.8, **0 split**.
2. **Test size decides the plan.** At 600 faces the LATERAL correctly plans a seq scan (it is
   cheaper there) and an index assertion fails on a good query; at 10 000 it plans the HNSW scan.
   The test seeds ~5000 for that reason — the same lesson the #127 module doc already recorded.

Purity — no cluster spanning two seeded identities — held throughout. That is the #124 property
restated at integration level.

### Was the index even the right call? Measured, because 49.6 s at the cap made it a fair question

A full pass over 20 000 faces takes **49.6 s**, which raised a real doubt: the in-memory scan is
quadratic but with a tiny constant, so at these sizes it might simply be faster, and step 2 would be
trading speed for scale nobody has. So it was measured rather than argued
(`in_memory_knn_cost_at_scale`, release build, same machine and vectors):

| faces | in-memory O(n²) | index-backed | speedup |
|---|---|---|---|
| 1 000  | 0.95 s  | —      | —    |
| 5 000  | 18.6 s  | 5.5 s  | 3.4× |
| 10 000 | 65.0 s  | —      | —    |
| 20 000 | 281.1 s | 47.7 s | 5.9× |

The index wins, and the gap widens with N. Two consequences fell out of the numbers:

- **The in-memory fallback is only safe at small N.** At 20 000 it would take 281 s — five times
  worse than the failure it covers for. It is now guarded by `IN_MEMORY_KNN_MAX = 2000` (the old
  cap, and the same reason it was that number).
- **`statement_timeout` was set too tight.** 60 s against a query measured at 47.7 s, with no usable
  fallback if it tripped. Raised to 180 s.

**Open, and a product call rather than a code one:** 49.6 s is correct but slow for an on-demand
triage screen. Options are to lower the cap (fast, but silently excludes faces again), to cache the
layout, or to compute it as a `work_jobs` job. Not decided here.

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
