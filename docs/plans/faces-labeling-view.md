# Faces view — cluster & bulk-label unknown faces

**Board card:** #119 Faces view — cluster + bulk-label unknown faces → people (In Progress)
**Follows:** [[face-recognition-pipeline]] (#117 — the pipeline that captures unknown faces).
**Goal:** Turn piles of unknown captured faces into named people. Overriding UX goal (owner): **label
MANY face photos at once, as simply as possible** — a cluster of similar faces is the unit of labeling;
one click/keystroke names the whole group. Optimize for rapid, low-friction triage.

**Decisions locked (owner):** real face **crops** (glyph only as fallback when a face has no image) ·
**grouped gallery** v1 (2D cluster canvas is a later follow-up) · **auto-suggest + confirm** (each
cluster pre-recognized against enrolled people) · label target = **new OR existing person**.

## The heart — a triage queue where one action labels a whole cluster
`/faces` loads clusters of unknown faces (largest first). Each card: representative **crop** hero +
"12 faces" + a pre-filled suggestion *"Looks like Ana · 88%"*. The primary `.press` button **is the
label** — **"Confirm Ana"** → one request labels+enrolls all N faces; no suggestion → **"Name this
person"** (inline input, not a modal). Keyboard: `Enter`/`Y` confirm · `E` someone else · `X` dismiss ·
`J`/`K` move — rip through with `Enter, Enter, Enter`; each keystroke commits a whole cluster (that's
"many at once"). Confirmed cards fade+collapse, focus advances. Per-card "Review N faces" disclosure to
exclude an outlier crop (Set-based selection — no checkbox component); a "select mode" labels several
clusters as one person (same identity split across clusters → merge-by-labeling, no merge editor).

## Phase 1 — app-core `faces` (migration + repo) · `crates/app-core/src/faces.rs`
- `embeddings_unlabeled(db,tid)`, `get`, `image_key_of`, `set_person(ids, Option)`, `delete(ids)->keys`,
  `list(filter,cursor,limit)` (unlabeled/person/camera; keeps `image_key`).
- **`cluster_unlabeled(db,tid) -> Vec<FaceCluster>`** — on-demand, Rust: **single-link connected
  components** (union-find) at **cosine ε = 0.40** (= recognition `max_distance`; unit-norm ArcFace →
  `dist = 1 - dot`), cap n (~2000). `FaceCluster { key, face_ids, representative, size, suggestion? }`,
  largest first. Suggestion = `persons::recognize(centroid,5)` + `best_match(_,0.40)` on the centroid.
- Migration **0062**: partial index `faces (tenant_id) WHERE person_id IS NULL`; optional soft `ignored`.
  **Validate on the live DB before deploy** ([[validate-migrations-live-db]]).
- **MinIO retention carve-out:** `media.rs::ensure_lifecycle` expires the shared staging bucket → face
  crops vanish. Store crops under a **`faces/` prefix** (`put_face`) + scope the lifecycle rule so
  `faces/*` is retained. One change into the StoreFace path (`workflows/node/vision.rs`).

## Phase 2 — control `sentinel-control/src/domain/faces.rs` (mirror `dictionaries.rs`; merge in `mod.rs`)
Reuse `persons.read`/`persons.write`. `GET /faces` · `GET /faces/clusters` · `GET /faces/:id/image`
(token-in-query proxy, tenant-gated, `spawn_blocking(media::get)` → jpeg bytes; mirror `run_frame`) ·
**`POST /faces/label`** (ONE tx: resolve/create person via `persons::repo::save` → `insert_embedding`
approved source="labeled" per face → `set_person`) · `POST /faces/reassign` · `DELETE /faces` (+ purge).

## Phase 3 — data layer
`types.ts`: `Face`/`FaceCluster`/`ClusterSuggestion`. `data.ts`: `listFaces`, `listFaceClusters`,
`faceImageUrl(id)` (token-in-query, copy `runFrameUrl`), `labelFaces`, `reassignFace`, `deleteFaces`.
`nav.ts`: `Faces → /faces`.

## Phase 4 — the view `sentinel-app/src/routes/faces/+page.svelte`
Dictionaries skeleton (PageHeader/Loader/EmptyState/IconChip, `.press`, `grid gap-4 sm:grid-cols-2
lg:grid-cols-3`; cells = cluster cards). Crop frame `rounded-md border bg-well` + `aspect-square
object-cover`; **PointCloudAvatar** dashed fallback. Active card = `border-primary bg-primary/10`. Full
route, never a modal. Impeccable: dot-field Loader, mark EmptyState, confirmed cards fade+collapse,
`toast.success`, confidence as **ink weight** (no colored bar). Labeled person shows **enrolled**.

## Verification
Migration 0062 (live-validated). Clusters group 2–3 seeded identities at ε=0.40, largest-first,
suggestions populate. Image proxy: 200 in-tenant / 404 cross-tenant. **Promote tx (critical):**
`POST /faces/label` → N approved embeddings + N stamped faces, all-or-nothing; new-person path creates a
`persons` row. Recognizes next time (held-out crop < 0.40; cluster leaves the queue). Retention survives.
UI drive: `Enter` through cards → toasts + queue decrement + enrolled; exclude a thumb → stays unknown.

## Open decisions (settle at build)
Ignore soft vs hard · retention `faces/` prefix vs bucket · ε fixed 0.40 vs slider · label embeddings
re-query vs trust ids · new-person id (uuid vs slug).

## Out of scope (v1)
2D cluster canvas / UMAP · a merge/split editor (approximated by multi-select) · re-cluster tuning UI ·
realtime updates · bulk relabel beyond single-face reassign.
