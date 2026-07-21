# Recognition Scoring — Per-Person Template + Margin Test — Implementation Plan

**Board card:** Recognition scoring — per-person template + margin test (ToDo)
**Goal:** Stop deciding `found` from the single nearest embedding. Aggregate distance per person and
add a top-1/top-2 margin gate so one lucky/unlucky crop can't dictate accept/reject.

## Why
`faces::recognize` (`crates/app-core/src/faces.rs:99`) does `UNION ALL … ORDER BY distance LIMIT k`,
so the match is whatever single row is nearest. Two failure modes:
- A person with many enrolled embeddings can win on one outlier crop; a single bad enrollment crop can
  also make a true person *lose*.
- A confident nearest neighbor that is barely closer than an equally-near **different** identity is
  exactly the false-accept case the flat 0.40 threshold can't see.
`found` = nearest LABELED within threshold ([[face-recognize-found-labeled]]) — keep that meaning,
improve the *score* behind it.

## Phases
1. **Per-person aggregation in SQL.** Rework the `recognize` query to `GROUP BY reference` and score
   each person by `min(distance)` *and* the mean of its top-3 nearest (return both). Keeps enrolled +
   stored UNION, still tags `source`.
2. **Margin test.** In `run_recognize_face` (`workflows/node/vision.rs`), require
   `dist(top1_labeled) + MARGIN < dist(top2_differentperson)` before `found=true`. Expose the margin
   (and the raw top-1/top-2 distances) so a graph can branch on ambiguity.
3. **Expose confidence + margin as output ports** on Recognize Face for downstream graph decisions
   (pairs with the upstream quality gate, [[face-quality-gate-upstream]]).
4. **Keep StoreFace a pure sink** — all thresholds/branching live in the graph, never in the node
   ([[storeface-pure-sink]]).
5. **Prototype against the live DB first** ([[validate-migrations-live-db]] discipline) — this is a
   query-shape change, measure recall/precision on real captures before wiring the node.

## Open decisions
1. `min` vs `mean-of-top-3` as the primary person score (or a blend). Decide from measured data.
2. Default MARGIN value — calibrate with [[face-threshold-calibration]].

## Out of scope (v1)
- Learned re-ranking / classifier head. Pure distance aggregation only.

## Related
[[face-pipeline-architecture]] · [[face-recognize-found-labeled]] · [[face-knn-hnsw-rerank]] · [[face-threshold-calibration]]
