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

## Phase 1 — Person module + hierarchy & permissions (foundation)

- Migration `0002_persons.sql`:
  - `persons(id, name, parent_id null, recipient_label null, embedding vector(512) null, consent bool default false, created_at)` — pgvector already enabled in `0001_devices.sql`.
  - `person_permissions(superior_id, action)` — actions a superior may perform over its subtree (e.g. `view_photo`, `view_location`, `view_report`, `receive_notifications`).
- `persons.rs` + `person_actions.rs` mirroring `devices.rs` / `device_actions.rs`: `PersonStore = Arc<Mutex<Vec<Person>>>`, `Person: sqlx::FromRow`, load/save in `db.rs`. Hierarchy helpers (`ancestors(id)`, `subtree(id)`) + a `can(superior, action, target)` permission check.
- Tauri commands (`list_people` / `save_person` / `delete_person` / `set_parent` / `link_recipient` / `grant_permission` / `revoke_permission`) + managed state wired in `src-tauri/src/lib.rs`. Enforce permissions at this layer.
- Frontend `/people/+page.svelte` (CRUD, set parent, link recipient, manage permissions) + nav link in `+layout.svelte`. shadcn dialogs — no `window.prompt`/`confirm` (no-ops in the WebView).

## Phase 2 — Face enrollment (biometrics)

- Local face pipeline in `agent/detector.rs`: `detect_faces()` (small face-detector ONNX, e.g. SCRFD/YOLO-face) + `embed_face() -> Vec<f32>` (512-d, ArcFace/MobileFaceNet) via tract — same in-process, embedded-model pattern as YOLOX. New model files under `agent/models/` with their LICENSE.
- Enrollment screen: capture a go2rtc frame → detect the largest face → compute embedding → `UPDATE persons.embedding`, set `consent=true`. New command `enroll_face(person_id, image)`.

## Phase 3 — Recognition + workflow nodes

- Node `RecognizePerson` (Vision): image → person id/name + confidence + ok. Executor: embed the face, then pgvector cosine kNN (`SELECT … ORDER BY embedding <-> $1 LIMIT k`); accept top-1 if within the distance threshold. Defaults: **k = 3**, cosine-distance threshold ≈ **0.4** (tune during build).
- Node `NotifyGuardians`: a recognized person → walks `parent_id` ancestors, resolves each guardian's recipient, sends the message/photo to all (reuses `notification::dispatch`). Honours `receive_notifications` permission.
- Canonical graph reuses existing nodes: `CameraSnapshot → RecognizePerson → If (ok) → Format({name},{time}) → NotifyGuardians`.
- Keep `agent/tools.rs` in sync if any new capability is exposed to the chat agent.

## Phase 4 — Frequency log + report (TBD in use case)

- `attendance(person_id, device_id, direction, recognized_at, confidence)`; log on each recognition; a light report view (per-person entries, per-day counts). Keep minimal in v1.

## To confirm during build

- Exact ONNX models for face detect + embed (and their licenses), pinned in `agent/models/`.
- Cosine-distance threshold + k, tuned on real enrollment samples.
- `direction` (in/out): derived per-device config (entrance vs exit) or a node input.

## Out of scope (v1)

- Per-viewer blur ("only your own child is visible") and full privacy/consent UI → v2.
- Parent chatbot ("meu filho está na escola?").
- Multiple embeddings per person (re-enrollment / averaging).
