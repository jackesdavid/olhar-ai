# Face Clustering — Kill Single-Link Chaining — Implementation Plan

**Board card:** Face clustering — kill single-link chaining (ToDo)
**Goal:** Replace the single-link union-find grouping so two different people can't be merged through
one ambiguous bridge face. Improves cluster purity in the Faces workspace.

## Why
`build_groups` (`crates/app-core/src/faces.rs:326`) is single-link connected components: if A~B and
B~C at ε, A and C merge even when A≁C. On surveillance faces this collapses distinct identities into
one island through a single borderline frame. It's also O(n²) capped at `MAX_CLUSTER_N = 2000`
(`faces.rs:238`). Clustering feeds both views (`cluster_unlabeled`, `layout_faces`) and the
centroid-based enrolled-person suggestion.

## Phases (cheapest → best)
1. **Mutual-kNN + min cluster size.** Only union i,j if each is in the other's top-k neighbours;
   drop singletons/tiny clusters below a min size. Kills most chaining with minimal code, still in
   `build_groups`.
2. **Average-link or complete-link agglomerative** instead of single-link — directly removes the
   transitive-merge pathology (merge clusters only if their *average*/*max* inter-distance ≤ ε).
3. **HDBSCAN** (density-based, no fixed ε) — biggest quality jump, handles varied cluster density,
   most work. Evaluate as the eventual target.
4. **Scale the neighbour graph via the index.** Build the kNN graph from the pgvector HNSW index
   ([[face-knn-hnsw-rerank]]) instead of all-pairs, so clustering scales past 2000 and the better
   algorithms stay cheap.
5. Keep centroid renormalization + largest-first ordering + suggestion logic (`build_cores`,
   `faces.rs:355`) intact; only the grouping changes. ε slider (`?eps=`) stays live on both views.

## Open decisions
1. k for mutual-kNN and the min cluster size — tune on real captures.
2. Whether to ship stage 1 now and defer HDBSCAN, or jump straight to average-link.

## Out of scope (v1)
- Per-tenant learned cluster params (belongs with [[face-threshold-calibration]]).

## Related
[[faces-clustering-and-proxy]] · [[faces-workspace]] · [[face-embed-landmark-align]] · [[face-knn-hnsw-rerank]]
