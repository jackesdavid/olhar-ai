# Face recognition pipeline — edge detect → embed → recognize → store

**Board card:** #117 Face recognition pipeline — edge detect → embed → recognize → store (ToDo)
**Supersedes/absorbs:** [[face-recognition-next-steps]] (#17 — the cloud `DetectFaces` node becomes
edge detection + a `face_detected` event; `RecognizeFace` generalizes `RecognizePerson`).
**Sibling modality:** [[reid-person-reidentification]] (#59 — same detect→embed→recognize→store shape
for *body* appearance).
**Goal:** The full face journey as composable workflow nodes: the **edge** detects faces and emits a
rich `face_detected` **event**; in the cloud a graph **extracts an embedding**, **recognizes** it by
kNN with tunable top-K, then **decides to store** the face — labeled (person X) or **unknown** — with
its **image + embedding in the tenant store**.

## The journey (owner, 2026-07-18)

```
[EDGE]  face detection ─▶ emit `face_detected` event  (frame, bbox, confidence, face/frame size ratio, camera)
                                     │
[CLOUD] OnEvent("face_detected") ─▶ Extract Face Embedding (256/512, model-abstracted)
                                     ─▶ Recognize Face (kNN top-K → matches + scores)
                                     ─▶ If(score …) ─▶ Store Face (labeled: person X | unknown)
                                                              image → MinIO, embedding → pgvector
```
"This is enough as the first phases." Later (out of scope here): auto-label + clusterize similar
faces + relate a cluster to a person — see *Out of scope*.

## Grounding (what exists to build on)

- **Edge:** pure-Rust `tract` inference (no OpenCV) — YOLOX person detector (`[[vision-detector-tract-yolox]]`,
  `agent/detector.rs`), motion (`sentinel-edge/src/motion.rs`), tracking (`tracking.rs`). **No face
  detector on the edge yet** — this adds one. Motion already **POSTs a normalized event with the JPEG
  frame to control** (`motion.rs:217`, box-token) → `OnEvent("motion")`; the face event mirrors that path.
- **System events:** `repo::system_events()` (`crates/app-core/src/workflows/repo.rs:461`) — `motion`
  carries `camera/score/area/blobs/bbox/frame`. Add `face_detected` here (same shape + confidence + ratio).
- **Embedding:** `agent/face.rs` — sidecar op `face_detect_embed` → **512-d L2-normalized ArcFace**
  (`EMBED_DIM=512`); ops `face_detect` / `face_embed` also exist (`ai/sidecar/handler.py`). `ai_service::call`.
- **Recognition:** `persons::recognize` (`persons/repo.rs:151`) — pgvector cosine `<=>` kNN over the
  tenant's **approved** `person_embeddings` (`vector(512)`, `0003_person_embeddings.sql`).
- **Storage:** `person_embeddings` requires a `person_id` (enrolled only) — **unknown faces need a new
  table.** MinIO via `media::put/get/delete` (`crates/app-core/src/media.rs`).
- **Nodes:** `RecognizePerson` (Image→best match), `BreakFaceDetection` (Unpack Face), `Face`/`FaceArray`
  port types exist. Node authoring pattern: `NodeKind::ALL` + ports + `node.rs` dispatch (just did the
  9 dictionary nodes). Next migration: **0061** (verify `ls crates/app-core/migrations/`).

## Phase order — cloud pipeline first (testable via a Snapshot image), edge detection last

### Phase 1 — Tenant face store (storage)
- Migration `0061_faces.sql`: `faces` — `id uuid`, `tenant_id`, `person_id text NULL` (**NULL = unknown**,
  else the label → FK `persons(id)`), `embedding vector(512)`, `image_key text` (MinIO), `camera text`,
  `bbox jsonb`, `confidence real`, `face_ratio real`, `source text`, `created_at text` (RFC3339).
  pgvector index (mirror `person_embeddings`); index `(tenant_id, person_id)`.
- `app_core::faces` module: `insert` (labeled/unlabeled), `recognize(tenant, embedding, top_k)` →
  top-K by cosine (over `faces` **and** `person_embeddings` — see decision 3), `list`, `label`(set
  person_id), `delete`. RFC3339 text timestamps.

### Phase 2 — Embedding representation + `Extract Face Embedding` node (abstraction)
- **`PortType::Embedding` + `PortValue::Embedding(Vec<f32>)`** (dim-agnostic float vector; typed edges,
  not a `NumArray` of i64). `portColors.ts` entry.
- Node **`FaceEmbedding`** (label "Extract Face Embedding"): in `image` (Image, req) + `bbox` (Bbox,
  optional — crop/align to the face); out `embedding` (Embedding), `dim` (Num), `ok` (Bool). Calls the
  sidecar via `agent/face.rs` (`face_embed` with the bbox/kps, else `face_detect_embed`). **Abstraction:**
  config `model`/`dim` so the provider/dim is swappable (512 today; 256 = decision 6).

### Phase 3 — `Recognize Face` node (kNN, parametrized)
- Node **`RecognizeFace`**: in `embedding` (Embedding, req); config `top_k` (default 5), `max_distance`
  (cosine threshold); out `person` (Text — best label or ""), `confidence` (Num, 0–100), `matches`
  (top-K as a **Dictionary** person→score, reusing #113, or a Table — decision 4), `found` (Bool).
  Calls `faces::recognize(tenant, embedding, top_k)`. Generalizes `RecognizePerson` (which stays,
  Image-in).

### Phase 4 — `Store Face` node (labeled or unknown)
- Node **`StoreFace`** (exec passthrough): in `image` (Image), `bbox` (Bbox opt), `embedding`
  (Embedding, req), `label` (Text/Recipient — a person id; **empty = unknown**); config: `dedup_distance`
  (skip if an existing embedding is within ε — decision 5). Persists: image → `media::put` (MinIO key),
  row → `faces` (`person_id = label or NULL`, embedding, image_key, bbox, confidence, camera, source).
  The graph decides labeled vs unknown by wiring `RecognizeFace.person` (or "") into `label`.

### Phase 5 — Edge face detection + `face_detected` event
- Edge: add a **tract face detector** (SCRFD-500m or YOLOX-face ONNX) mirroring `agent/detector.rs`;
  run **motion-gated** (like the tracker) so it's cheap. On detect, POST a `face_detected` event to
  control (same seam as `motion.rs:217`) with: `frame` (JPEG), `bbox`, `confidence`, `face_ratio`
  (face area ÷ frame area), `camera`.
- `system_events()`: register `face_detected` (fields mirror `motion` + `confidence` + `face_ratio`),
  so `OnEvent("face_detected")` exposes typed outputs immediately.
- **This is the heaviest phase** (edge inference) — sequenced last so Phases 1–4 ship a working cloud
  pipeline first, testable with a `Snapshot`/manual image feeding `FaceEmbedding`.

## Decisions — LOCKED (owner, 2026-07-18)

1. **Embedding port type** → a new **`PortType::Embedding` + `PortValue::Embedding(Vec<f32>)`** (typed
   edges, not an i64 `NumArray`). One `Embedding` type; the model/dim is the producer's concern.
2. **Detection location** → **ON THE EDGE (tract)** — a real edge face detector (SCRFD/YOLOX-face,
   motion-gated) emits `face_detected` with a true bbox. This is Phase 5, built **last**: Phases 1–4
   ship the cloud pipeline first, tested with a `Snapshot`/manual image, then the edge feeds real events.
3. **Recognition gallery** → **union of `person_embeddings` (enrolled) + `faces` (all stored, labeled +
   unknown)**, each match tagged `source = enrolled | stored`, so unknown faces match prior unknowns and
   clusters form before anyone labels them.
4. **Top-K output** → a **#113 Dictionary** (`person → score`) for `matches`, plus best-match scalars
   (`person`, `confidence`, `found`).
5. **Store dedup** → `StoreFace` has a `dedup_distance` config (default ON): skip an insert when an
   existing embedding is within ε cosine, so a lingering face can't flood the store.
6. **Embedding dim** → **512** (ArcFace, reuse `vector(512)` — the enrolled gallery and the new `faces`
   table share one comparable size; a fixed vector size is required for the union kNN). The node keeps a
   `dim`/`model` config for a future swap, but the store commits to 512.

## Out of scope (next phases — the "after this works" work)

- Views to **auto-label** and **clusterize** similar faces (2D cluster canvas — mirrors #59's later phases).
- Relating a cluster/unknown face to a **person** (promote unknown → labeled, merge).
- Review/triage queue; per-face thumbnails gallery.
- GPU sidecar move (RTX 2060) — ops, tracked in #17 Phase 3.
