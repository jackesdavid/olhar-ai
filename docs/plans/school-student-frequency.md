# School Student Frequency — Implementation Plan

**Use case:** [[school-frequence]]
**Board card:** School Student Frequency Case (ToDo)
**Goal:** recognize a person at an entrance/exit camera and notify their parent/guardian.

This plan is grounded in what already exists in the codebase: pgvector (`app/crates/core/src/db.rs`,
`migrations/0001_devices.sql`), the devices store pattern, the local YOLOX/tract inference path
(`agent/detector.rs`), the notification recipients module (`notification/`), and the workflow node
registry (`workflows/domain.rs` + `node.rs`).

## Decisions (locked)

1. **Face inference: local, bundled (tract).** A face detector + embedding ONNX model live under
   `agent/models/` and run in-process via tract — same pattern as YOLOX. No sidecar, no cold start,
   works offline. Embedding dimension **512** (ArcFace/MobileFaceNet-style) → `vector(512)` column.
2. **Hierarchy: full, generic + permissions.** A generic superior→subordinate tree (parent→child,
   boss→employee), with per-superior permission actions over its subtree. Kept domain-agnostic.
3. **Notify resolution: walk the hierarchy.** Each Person has at most **one** linked recipient
   (their own device). To notify a recognized student, walk up `parent_id` to all guardians and send
   to each guardian's recipient. Multiple parents fall out of the tree — no person↔recipient table.
4. **Privacy/blur: deferred to v2.** v1 keeps a `consent` flag captured at enrollment and enforces
   permissions at the command layer. The per-viewer "blur everyone except your own child" rendering
   and the full consent/privacy UI are v2.

## Phase 1 — Person module + hierarchy & permissions (foundation) ✅ DONE

Shipped:
- Migration `0002_persons.sql`: `persons(id, name, parent_id null → ON DELETE SET NULL, recipient_label null, permissions text[], embedding vector(512) null, consent bool, created_at)` + a `parent_id` index. pgvector was already enabled in `0001_devices.sql`.
- `crates/core/src/persons.rs`: `Person` (`sqlx::FromRow`), `PersonStore = Arc<Mutex<Vec<Person>>>`, `PERMISSIONS` constant, and helpers `find` / `ancestors` / `subtree` / `would_cycle` / `can(superior, action, target)` / `guardian_recipients` — with unit tests.
- `db.rs`: `load_persons` (scalar cols only — embedding read separately) + `save_persons` (UPSERT + prune, so an enrolled embedding survives a metadata edit).
- Tauri commands in `src-tauri/src/lib.rs`: `list_people`, `person_permission_kinds`, `save_person` (validates + cycle-guards), `delete_person` (detaches children); `PersonStore` managed + hydrated in `try_connect_db`.
- Frontend: `/people/+page.svelte` is a directory table (rows open the record); `/people/[id]/+page.svelte` is a **full-page record screen** (`new` = create) with Details / Hierarchy (guardian + subordinates) / Recipient / Permissions / Biometrics-placeholder cards — chosen over a modal so the record can grow (enrollment, reports). Nav link in `lib/nav.ts` (Users icon). Delete confirm stays a small dialog; no `window.prompt`.

- Search-by-name on the directory list (client-side, case-insensitive; also matches recipient label).

Deviations from the original plan (simpler, equivalent):
- **Permissions as a `text[]` column on `persons`**, not a separate `person_permissions` table — one table, one store; permissions travel with the superior.
- **CRUD folded into `save_person`** (full-object upsert, like `save_device`) instead of separate `set_parent` / `link_recipient` / `grant_permission` / `revoke_permission` commands. Hierarchy cycles are guarded server-side.

## Phase 2 — Face enrollment (biometrics) ✅ DONE (vision backend = placeholder)

Shipped:
- `crates/core/src/agent/face.rs`: `embed(image) -> Vec<f32>` returning an L2-normalised, `EMBED_DIM` (512) vector, with unit tests. Single documented swap-point for the real model.
- `db.rs`: `enroll_face(id, &[f32])` (`UPDATE … embedding = $1::vector, consent = true`) + `clear_face(id)`. Embedding bound as a pgvector text literal (`[…]::vector`) — no extra crate.
- `src-tauri/src/lib.rs`: `enroll_face(id, image_b64)` (decode → `face::embed` → store → flip in-memory consent) + `clear_face(id)` commands.
- Record page Biometrics card: upload a photo → enroll (data-URL → backend), shows enrolled/consent state, Re-enroll + Remove. Gated until the person is saved (needs a DB row).

⚠️ **Placeholder vision backend.** `face::embed` currently resizes the whole frame to a small grayscale thumbnail (deterministic, content-derived) — it makes the enroll → store → cosine-kNN → notify slice work end to end, but it is **not** real face recognition. To make it real: bundle a face detector + ArcFace/MobileFaceNet ONNX under `agent/models/` and replace the body of `face::embed` (detect largest face → align/crop → run model → L2-normalise). The `vector(512)` column, DB, commands, and UI all stay as-is. *(I can't fetch/bundle the model in this environment — drop the ONNX in and it's a one-function change.)*

Decision: **photo upload is kept as a secondary enrollment method.** The primary path
(camera capture) and multi-face support are the next phase — the v1 single-embedding column will be
superseded by a per-face table there.

## Phase 3 — Multi-face enrollment + camera capture ✅ DONE (vision backend still placeholder)

Shipped: migration `0003_person_embeddings.sql` (new table; dropped `persons.embedding`/`consent`);
`PersonEmbedding` type + `db` per-row CRUD (`insert_embedding` / `list_embeddings` /
`approve_embedding` / `delete_embedding` / `enrolled_person_ids`); commands of the same names
(`enroll_face` now inserts a row with `source` + `approved`); a dedicated enrollment **route**
`/people/[id]/enroll` (camera picker over saved cameras → `capture_snapshot`, upload fallback,
approve-on-capture toggle, per-sample approve/remove); `PointCloudAvatar` component rendering each
embedding as a dot-field (no photo stored); the record page Biometrics card is now a summary +
"Open enrollment"; the directory's "Enrolled" badge uses `enrolled_person_ids`. Decisions resolved:
the enrollment "window" is an in-app **route** (not a separate OS window); the point-cloud is a
**min-max paired-component projection** of the 512-d vector. *(The `face::embed` backend is still
the Phase 2 placeholder — swapping in a real model stays a one-function change.)*

Original spec below:



Goal: enroll **several faces per person** (different angles/lighting) for more robust recognition,
make **camera capture the primary** way to enroll (photo upload stays as the secondary fallback),
and visualise each enrollment **without ever storing the photo** (LGPD).

- **Privacy by design (LGPD): store only the embedding, never the source image.** The captured
  frame is used to compute the embedding in-memory and then discarded — nothing in the DB or on
  disk is the person's photo.
- **Schema: a dedicated embeddings table — one row per sample, not a field on `persons`.**
  Migration `0003_person_embeddings.sql`:
  `person_embeddings(id uuid PRIMARY KEY, person_id text → persons ON DELETE CASCADE, embedding vector(512) NOT NULL, approved boolean NOT NULL DEFAULT false, approved_at timestamptz, source text, created_at timestamptz DEFAULT now())`
  with an index on `person_id`. **N rows per person**, each linked by `person_id`, and the
  **approval lives on the row** (per-sample consent). This **retires `persons.embedding` and
  `persons.consent`** — a person is "enrolled" when they have ≥1 **approved** embedding (derive it,
  don't duplicate a person-level flag).
- Backend: `enroll_face` **inserts** a `person_embeddings` row (uuid generated server-side, `approved` per the consent toggle at capture), not an overwrite; add `list_embeddings(person_id)`, `approve_embedding(id)`, `delete_embedding(id)`; `db` gains the per-row CRUD. Multiple rows make the recognition query a single kNN over the whole table.
- Camera capture (primary): the enrollment window has a **camera picker** — choose which **saved camera** (from the `DeviceStore`, defaulting to the last-used one) to enroll from; grab a go2rtc frame from that device (reuse `core::go2rtc` / `capture_snapshot`) → detect the largest face → embed → enroll. **Saved cameras only** (no local webcam). Photo upload stays as the secondary path.
- **Per-enrollment visualisation — a "point-cloud avatar" derived from the embedding**, not the
  photo. Each `person_embeddings` row renders as a deterministic dot-field glyph (project the 512-d
  vector to 2D/3D points; dot position/size/colour seeded by the vector), so every sample is shown
  as a recognisable-yet-abstract fingerprint. Leans on the product's existing **dot-field visual
  identity** (see the `impeccable` skill) — and shows *something* per enrollment with zero PII.
- **Dedicated enrollment window**: a separate full-screen window/route (not a card on the record
  page) that owns the whole flow — live camera preview + capture, the gallery of point-cloud
  avatars for every enrolled sample, per-sample remove, and the sample count. The record page's
  Biometrics card becomes a summary (count + "Open enrollment") that launches this window.

> **The use case is a graph, not a feature.** School-frequency is delivered as a *saved workflow
> graph* composed from the **generic, reusable nodes** below — nothing in the code is school-specific.
> The phases add the missing general-purpose nodes; the use case is then wired in the editor.

## Phase 4 — Recognition node (generic) ✅ DONE (vision backend still placeholder)

Shipped: `RecognitionMatch`/`RecognitionResult` + `persons::best_match` (closest within
`max_distance`, confidence rewards extra supporting samples, unit-tested); `db.recognize(embedding, k)`
(cosine kNN over **approved** `person_embeddings`, joined to `persons`); the generic **`RecognizePerson`**
Vision node (`image → person / person_id / confidence / ok`, `max_distance` config) + executor + debug
summary + "requires image" test; a direct **`recognize_face`** command (returns the query embedding + best match). Not exposed as a
chat tool yet (no `agent/tools.rs` change).

UI reorg: recognition testing moved off the person record onto a **dedicated screen**
`/people/recognize` — pick a saved camera or upload a photo, then a **big side-by-side point-cloud
comparison** (target vs. the recognised person, with name + confidence). `/people` is now a **People
dashboard** (tiles → Directory / Recognition); the list lives at `/people/directory`; record +
enrollment routes unchanged. Nav "People" → the dashboard.

**Real face pipeline implemented + models fetched + validated running.** `agent/face.rs` runs the
real pipeline via tract: **SCRFD** (`scrfd.onnx`, 640², `(x−127.5)/128` RGB, top-scoring box, no NMS) →
crop → **ArcFace** (`arcface.onnx`, 112² RGB `(x−127.5)/127.5`) → L2-normalised 512-d. Models load at
**runtime** from `SENTINEL_MODELS_DIR` (Tauri shell → `<app_data>/models`); absent/invalid → the
deterministic **placeholder** (logged), so the app works offline — drop the files in, no rebuild.

- **Pivot:** YOLOv8-face → **SCRFD**. No YOLOv8-face ONNX is fetchable (those repos ship `.pt`;
  export needs torch), whereas the InsightFace **`buffalo_l`** pair ships as ONNX and is ArcFace's
  native detector. Fetched from `huggingface.co/immich-app/buffalo_l` (detection=SCRFD ~17 MB,
  recognition=ArcFace w600k_r50 ~174 MB) into `<app_data>/models`.
- **Validated** via `cargo run -p app-core --example face_probe`: both models load/optimise in tract,
  the SCRFD decode runs, ArcFace returns **512-d, norm 1.0000**. ⚠️ Still to confirm with a *real face
  photo*: detection box quality and recognition discrimination (synthetic probe only proves
  load/shape/decode). First call optimises the 174 MB model (~17 s) then caches — consider a startup/
  screen-entry warmup. ArcFace **RGB vs BGR** still worth a sanity check on real faces.
- Node, commands, `vector(512)` column, and `EMBED_DIM` unchanged.

## Phase 5 — Composable fan-out notification (generic nodes + loop primitive)

> **Decomposed in its own note:** [[foreach-and-fanout-notification]] — the `List` port type,
> `GetPersonParentRecipients`, the `ForEach` node + engine-iteration design options, and tests.
> Settle the engine approach there before building (it's the heaviest, riskiest piece).

Notify every guardian by **composing generic nodes**, not a dedicated "notify guardians" node. Flow:
`RecognizePerson → If (recognized) → GetPersonParentRecipients → ForEach → (body) Format → NotifyRecipient`.

- **New port type — a list/collection** (e.g. a list of `Recipient`) so a producer can fan a set of values into an iterator. Add a `List` variant to `PortType` (carrying its element type).
- Node `GetPersonParentRecipients` (generic): input a person (id) → output a **list of recipients** = the person's ancestors that hold `receive_notifications` and have a recipient (reuses `persons::guardian_recipients`).
- Node `ForEach` / `Loop` (generic control-flow): input a list → fires its **body** exec once per element, exposing the **current item** as a typed output, then a **done** exec when the list is exhausted. ⚠️ **Engine change**: the engine is a single-pass *acyclic* DAG today (topo order, one run per node) — iteration means running the body subgraph once per item with the item bound, plus a sane iteration bound. Real work; this is why it's its own phase.
- Reuse existing `If`, `Format`, and **`NotifyRecipient`** (notify already exists) at the tail of the loop body.
- Keep `agent/tools.rs` in sync if exposed to the chat agent.

## Phase 6 — Attendance / frequency log + report (TBD in use case)

- `attendance(person_id, device_id, direction, recognized_at, confidence)` + a generic `LogAttendance` node; log on each recognition; a light report view (per-person entries, per-day counts). Keep minimal in v1.

## Phase 7 — The school-frequency graph (assembly)

- Wire the generic nodes into the canonical graph above and ship it as a **starter/template graph** (or document the wiring) — the use case's only "school-specific" artifact is this saved graph, not code.

## To confirm during build

- Supply `yolov8_face.onnx` + `arcface.onnx` (+ licenses) into `SENTINEL_MODELS_DIR` (`<app_data>/models`), then **validate the export-specific bits** in `face.rs`: detector output layout/score activation and ArcFace RGB-vs-BGR. (Pipeline is implemented; only the weights + validation remain — can't fetch/run them here.)
- Cosine-distance threshold + k, tuned on real enrollment samples.
- `direction` (in/out): derived per-device config (entrance vs exit) or a node input.

## Out of scope (v1)

- Per-viewer blur ("only your own child is visible") and full privacy/consent UI → v2.
- Parent chatbot ("meu filho está na escola?").
