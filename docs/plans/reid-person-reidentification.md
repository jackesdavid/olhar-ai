# Person Re-Identification (ReID) — Implementation Plan

**Board card:** Person Re-Identification (ReID) — body-appearance matching (ToDo)
**Goal:** Match detected people against a small per-tenant gallery by **body appearance**
(clothing/shape embedding, *not* face), label them with an identity + confidence, and grow the
gallery through a **two-phase, human-in-the-loop enrollment**: a dedicated opt-in `AutoEnroll`
node captures body crops (stored in tenant-scoped MinIO) with a state machine — high confidence →
auto-enrolled, mid → `probably` (awaits user confirm), low → `unknown` fallback — reviewed in a
per-person image gallery + triage queue and explored in a **cluster canvas** that projects the
embeddings to 2D. Per-tenant thresholds throughout. Reuses Sentinel's existing face-recognition,
S3/MinIO, and workflow-node infra end to end.

> This plan re-homes a generic "BISHOP / YOLO11n / ort+CUDA / Supabase / IBBF" draft onto
> **Sentinel's real architecture**. The corrections are deliberate — see *Architecture fit* below.

---

## Architecture fit — what changes vs. the generic draft

| Generic draft | Sentinel reality (what we actually do) |
|---|---|
| "BISHOP", YOLO11n + `ort` + CUDA EP | Detection is **YOLOX via `tract`** (local, BGR + no-normalize) — [[vision-detector-tract-yolox]]. On-edge multi-object tracking is a **pure-Rust SORT/ByteTrack** core (commits P1–P3). |
| Run OSNet locally alongside YOLO on the edge | **DECIDED: runs on the cloud AI sidecar** — a new `reid_embed` op in `ai/sidecar/handler.py` (a `def reid_embed` + one `OPS` entry + lazy `_reid_model()` loading OSNet beside InsightFace), called via `ai_service::call` exactly like `face_detect_embed` (mirrors `agent/face.rs`). Person crop is sent to the sidecar (accepted egress). *(Edge-local OSNet via `tract` — mirroring `agent/detector.rs` — is parked as a possible v2 optimization; `tracking.rs` carries the "cross-session re-ID … later phase" TODO.)* See decision #5. |
| New `tenants` + `people` tables, UUID PKs | **Reuse existing `persons`** (`crates/app-core/migrations/0002_persons.sql`, PK `id text`, app-generated) — a person already carries face embeddings; ReID just adds a second modality. `tenant_id` is already on every domain table (migration 0011). **Do not create `tenants`/`people`.** |
| `reid_embeddings` w/ `vector(512)`, `TIMESTAMPTZ` | Mirror the real **`person_embeddings`** table (`0003_person_embeddings.sql`): new sibling table `person_reid_embeddings`, `vector(512)`, FK → `persons(id)`, `approved` gate, `tenant_id`. **Timestamps as RFC3339 `text`** (`app_core::alerts::now_rfc3339()`) — sqlx is built w/o chrono/time, can't decode `timestamptz`. |
| Supabase pgvector, "check HNSW version" | Self-hosted Postgres on the VPS (`sentinel-postgres`), pgvector already in prod for `person_embeddings` + devices. Match is the existing `<=>` cosine kNN pattern (`persons::recognize`, `persons/repo.rs:153`) — an **`ivfflat`/plain index like the current table**, not necessarily HNSW. Gallery is a few hundred vectors/tenant → the existing bounded-timeout kNN is plenty; no in-memory cache. |
| Axum `/reid/*` on an unnamed backend; "IBBF conventions" | Endpoints live in **`sentinel-control`** (owns Postgres + `persons` CRUD); the workflow node lives in **`sentinel-flow`/app-core**. Frontend follows Sentinel conventions — `lib/data.ts` dual-mode client, shadcn dialogs (no `window.prompt`), monochrome ink theme. |
| Per-track ByteTrack smoothing in the recognizer | Track-level identity is a **v2** that enriches the existing `object_entered`/`object_exited` events (edge tracker + track-event relay), per the standing rule *"a new event-driven feature is a new event + gating inputs on OnEvent, never a new node kind"* ([[workflow-engine-loops-events]], `docs/ref/workflows.md`). v1 is a pull-based action node. |

Next migration number is **0038** (`0037_tracking_config.sql` already exists — CLAUDE.md's "next 0037" is stale).

---

## Phase 1 — Sidecar `reid_embed` op (OSNet-x0.25)

- `ai/sidecar/`: add a `reid_embed` op beside `face_detect_embed`/`face_detect`/`face_embed`
  (same dispatch/router). Load **OSNet-x0.25 ONNX** once at startup (a couple MB) next to the
  InsightFace `buffalo_l` session; reuse the CPU/GPU image split (`compose.gpu.yml`).
- **Runtime:** the cloud sidecar is **RunPod Serverless on an RTX A4500** (flex / scale-to-zero) —
  so OSNet runs GPU-accelerated in prod. Bake the OSNet ONNX into the `:gpu` image (and `:cpu` for
  the VPS fallback). Because ReID fires *per person per frame* (unlike enrollment-time face calls),
  A4500 throughput + RunPod's warm-worker behaviour is the thing to watch — keep at least one warm
  worker if graphs identify continuously, or throttle upstream.
- Input: `{ "image_b64": <person crop JPEG> }` (mirrors `ai_service::image_b64`). Preprocess:
  resize crop → 256×128, ReID normalization, NCHW → forward → **512-d** float vector →
  **L2-normalize**. Output `{ "embedding": [f32; 512] }`.
- Keep it embedding-only (no detection): the crop is produced upstream from a `bbox`, so the
  sidecar just embeds. This matches the `face_embed` op shape.

## Phase 2 — Rust: embed client + gallery match (app-core)

- **`crates/app-core/src/agent/reid.rs`** (new, mirrors `agent/face.rs`): `embed(crop_bytes) ->
  Result<Vec<f32>, String>` calling `ai_service::call("reid_embed", json!({ "image_b64":
  ai_service::image_b64(crop) }))` and returning the 512-d vector. Wire it into `agent/mod.rs`.
- **`person_reid_embeddings` table** — migration `0038_person_reid_embeddings.sql`, cloned from
  `0003` **but with the composite tenant FK from `0011`** (`persons` PK is `(tenant_id, id)`, so a
  bare `REFERENCES persons(id)` will not compile against the schema):
  ```sql
  CREATE TABLE IF NOT EXISTS person_reid_embeddings (
      id            uuid PRIMARY KEY DEFAULT gen_random_uuid(),
      tenant_id     uuid NOT NULL,
      person_id     text,                         -- NULLABLE: NULL while state='unknown' (unassigned)
      embedding     vector(512) NOT NULL,
      -- Review lifecycle (see §Enrollment state machine). Only 'auto_enrolled' + 'confirmed'
      -- set approved=true and thus count in recognition kNN.
      state         text NOT NULL DEFAULT 'probably',  -- auto_enrolled|probably|unknown|confirmed|rejected
      approved      boolean NOT NULL DEFAULT false,     -- derived from state; the kNN filter
      approved_at   text,                         -- RFC3339 (text — sqlx can't decode timestamptz)
      image_key     text,                         -- MinIO object key for the crop (tenant-scoped); NULL if not stored
      source        text NOT NULL DEFAULT '',     -- 'camera' | 'upload' | 'track:<uuid>'
      model_version text NOT NULL DEFAULT 'osnet_x0_25',  -- stamp from day 1 (face's known gap)
      confidence    real,                         -- similarity at enrollment/identify time
      captured_at   text NOT NULL DEFAULT '',     -- now_rfc3339()
      FOREIGN KEY (tenant_id, person_id) REFERENCES persons(tenant_id, id) ON DELETE CASCADE
  );
  CREATE INDEX IF NOT EXISTS person_reid_embeddings_person_id_idx ON person_reid_embeddings (person_id);
  CREATE INDEX IF NOT EXISTS person_reid_embeddings_tenant_idx    ON person_reid_embeddings (tenant_id);
  CREATE INDEX IF NOT EXISTS person_reid_embeddings_state_idx     ON person_reid_embeddings (tenant_id, state);
  ```
  A composite FK with a **nullable** `person_id` is valid (Postgres skips the FK check when any key
  column is NULL) — so an `unknown` sample sits in the same table, unassigned, until the user
  triages it. `person_reid_config` (per-tenant thresholds) is a JSONB table added in the same
  migration — see Phase 4.
- **`persons::recognize_body(db, tenant_id, embedding, k)`** in `persons/repo.rs` — copy
  `recognize()` (`persons/repo.rs:153`) verbatim but over `person_reid_embeddings` (same
  `(embedding <=> $1::vector)::float4 AS distance`, `WHERE approved AND tenant_id`, `SET LOCAL
  statement_timeout`, join `persons` for name). Bind the vector via the existing
  `vector_literal()` text-`$1::vector` trick (`persons/repo.rs`). Add a matching method on the
  `Recognizer` trait (`ports.rs:60`, impl `LocalRecognizer` ~`ports.rs:171`) so `ToolContext` can
  reach it — same pattern `sys.recognizer.recognize` already uses.
- Reuse `persons::best_match(&matches, max_distance)` for thresholding.

## Phase 3 — `RecognizeBody` workflow node (the "ReID node")

Mirror `RecognizePerson` (`domain.rs` variant, `run_recognize_person` at `vision.rs:105`) but
**crop by bbox** so it consumes person-detection output directly.

- `NodeKind::RecognizeBody` in `domain.rs`: add to enum + `ALL`, `slug="recognize_body"`,
  `label="Recognize Body"`, category `Vision`.
  - Input ports: `image*` (Image), `bbox*` (Bbox) — bbox is `PortType::Bbox` canonical xyxy
    ([[workflow-bbox-typed]]); comes from `Detect Objects` → `Unpack Person`
    (`ObjectDetection` → `BreakPersonDetection`).
  - Output ports: `person` (Text), `person_id` (Text), `confidence` (Num), `ok` (Bool) — same as
    `RECOGNIZE_PERSON_OUT` (`domain.rs:641`).
  - `default_config`: `{ "max_distance": "0.35", "timeout_secs": "30" }` (mirrors
    `RecognizePerson` at `domain.rs:1331`).
- `run_recognize_body(config, inputs, sys)` in `vision.rs`: decode image, crop to `bbox`,
  `reid::embed(crop)` → `sys.recognizer.recognize_body(&emb, 5)` → `best_match` → emit
  person/person_id/confidence/ok. Add the `run_node` dispatch arm (`node.rs:415` neighbourhood)
  + a `node_log_summary` arm + include in the sidecar-timeout group (`domain.rs:1077`).
- Catalog auto-derives from `NodeKind::ALL`; **no `agent/tools.rs` change** ([[keep-agent-tools-in-sync]]
  is about the chat agent, which doesn't list workflow nodes).
- Chain this enables: `Detect Objects(person) → For each → Unpack Person → (bbox) Recognize Body
  → RecordPresence / Notify`.

## Phase 4 — Enrollment domain: state machine, MinIO crops, per-tenant thresholds

This is the backbone the AutoEnroll node (Phase 5) and Review UI (Phase 6) both build on.

### Enrollment state machine (the `state` column)

Every stored body sample carries one of five states. **Two phases of auto-enrollment**, exactly
as requested — a sample is only *provisionally* stored until confidence (or a human) promotes it:

| `state` | Set when | `approved` (counts in kNN) | `person_id` |
|---|---|---|---|
| `auto_enrolled` | identify similarity ≥ `auto_enroll_min` (high) | **true** | the matched person |
| `probably` | `probably_min` ≤ similarity < `auto_enroll_min` | false | best-guess person |
| `unknown` | similarity < `probably_min` (fallback) | false | **NULL** (unassigned) |
| `confirmed` | user confirms a `probably`/`unknown` → this person | **true** | confirmed person |
| `rejected` | user rejects the sample | false | — |

- `approved` is **derived** from state (`auto_enrolled`/`confirmed` → true) and is what the
  `recognize_body` kNN filters on — so `probably`/`unknown` samples are stored and reviewable but
  never pollute matching until a human (or high confidence) promotes them. This is the two-phase
  gate: *stored provisionally → promoted to the gallery only on confirmation/high confidence.*
- **Reassign** (user says "that's actually a different person") just repoints `person_id` and sets
  `state='confirmed'`, and logs a row in `person_reid_corrections`
  (`predicted_person_id`, `corrected_person_id`, `embedding`, `corrected_at text`) as the audit
  trail / hard-negative signal.

### MinIO crop storage (tenant-scoped) — reverses the face feature's vectors-only stance

The user wants the actual crop images kept so the Review UI (Phase 6) can show them.

- Reuse the existing S3/MinIO client **`crates/app-core/src/media.rs`** (`media::put(bytes,
  "image/jpeg", "jpg")` / `media::get(key)` / `media::enabled()`, bucket `sentinel-media`).
- `media::put` today mints a flat `uuid.jpg` key — add a **tenant-scoped** variant
  `put_scoped(prefix, …)` (or pass a `reid/<tenant_id>/<uuid>.jpg` key) so crops are namespaced
  per tenant. Store the returned key in `person_reid_embeddings.image_key`.
- Serve crops back through a control route (Phase 6) that re-checks the caller's tenant before
  `media::get` — never hand the browser a raw bucket URL (keeps tenant isolation + auth).
- If `media::enabled()` is false (MinIO unset), degrade to vectors-only (no thumbnail) rather than
  failing the enroll.

### Per-tenant thresholds (`person_reid_config`)

Follow the `tracking_config` JSONB precedent (`0037`, on `tenant_boxes`) — a small per-tenant
JSONB row, evolvable without per-field migrations:

```sql
CREATE TABLE IF NOT EXISTS person_reid_config (
    tenant_id uuid PRIMARY KEY,
    config    jsonb NOT NULL DEFAULT
      '{"auto_enroll_min":0.90,"probably_min":0.75,"max_distance":0.35,
        "auto_enroll_enabled":false,"diversity_max_sim":0.95}'
);
```

- `auto_enroll_min` / `probably_min` are the two-phase similarity gates; `max_distance` is the
  `recognize_body` match threshold; `auto_enroll_enabled` defaults **off** (opt-in per tenant, given
  clothing-driven false positives); `diversity_max_sim` skips near-duplicate inserts.
- Control endpoints `GET/PUT /persons/reid/config` (RBAC `settings.read`/`settings.write`,
  `tenant_of(&c)`); a Settings-page section edits them. The AutoEnroll node + `recognize_body`
  read this per run (loaded into `ToolContext` like devices/thresholds already are).

### Repo + endpoints (mirror the face lane in `persons/repo.rs` + `domain/persons.rs`)

- `insert_reid_embedding(db, tenant, person_id: Option<&str>, emb, state, image_key, confidence,
  source)` — `INSERT … $N::vector …` with `approved` derived from `state`; mirrors
  `insert_embedding` (`persons/repo.rs:67`, text `vector_literal` bind).
- Review/triage endpoints under `sentinel-control/src/domain/persons.rs` (top-level, merged
  `domain/mod.rs:33`): `GET /persons/:id/reid-embeddings`, `GET /persons/reid/pending`
  (state in `probably`/`unknown`, tenant-scoped), `POST /persons/reid-embeddings/:eid/confirm`
  (→ `confirmed`), `POST …/reassign` (repoint `person_id` + correction), `POST …/reject`,
  `GET /persons/reid-embeddings/:eid/image` (tenant-checked `media::get`).

## Phase 5 — `AutoEnroll` workflow node (dedicated, opt-in per graph)

A **separate node** from `RecognizeBody` so a graph decides *when* growth happens — you can
identify without enrolling, or wire AutoEnroll only on trusted cameras/times.

- `NodeKind::AutoEnrollBody` in `domain.rs` (enum + `ALL`, `slug="auto_enroll_body"`,
  `label="Auto-Enroll Body"`, category `Vision`).
  - Input ports: `image*` (Image), `bbox*` (Bbox), `person_id` (Text, optional — the
    `RecognizeBody` output), `confidence` (Num, optional — same). Wiring
    `RecognizeBody.person_id/confidence → AutoEnroll` is the intended chain.
  - Output ports: `state` (Text: auto_enrolled|probably|unknown|skipped), `embedding_id` (Text),
    `ok` (Bool).
  - `default_config`: `{ "timeout_secs": "30" }` (thresholds come from `person_reid_config`, not
    the node, so they stay tenant-consistent).
- `run_auto_enroll_body(config, inputs, sys)` in `vision.rs`:
  1. Crop `image` to `bbox`, `reid::embed(crop)` (reuse the same embed path as `RecognizeBody`).
  2. Resolve `confidence`/`person_id` — from the wired inputs, else run `recognize_body` here.
  3. **Gate** against the tenant's `person_reid_config`: ≥ `auto_enroll_min` → `auto_enrolled`
     (subject to the diversity check: skip if max similarity to that person's own rows ≥
     `diversity_max_sim`); `probably_min`..`auto_enroll_min` → `probably`; below → `unknown`
     (person_id NULL). If `auto_enroll_enabled=false`, high-confidence still records as `probably`
     (never silently grows the gallery) — the toggle gates *auto-approval*, not capture.
  4. Store the crop to MinIO (`image_key`) + `insert_reid_embedding` with the chosen state.
- Dispatch arm in `node.rs`, log summary, sidecar-timeout group. Catalog auto-derives; no
  `agent/tools.rs` change.
- Chain: `Detect Objects(person) → Unpack Person → Recognize Body → Auto-Enroll Body`.

## Phase 6 — Review UI (per-person body images + triage)

A view to see a person's body images with their auto-enrollment states, and act on them.

- **Per-person gallery** — extend `sentinel-app/src/routes/people/[id]/` with a "Body" tab:
  thumbnail grid of that person's `person_reid_embeddings` (image via the tenant-checked
  `/persons/reid-embeddings/:eid/image` route), each tagged with its `state`
  (auto_enrolled / probably / confirmed) and confidence.
- **Triage queue** — a tenant-wide `probably` + `unknown` list (`GET /persons/reid/pending`),
  each card showing the crop with actions, all via `lib/data.ts` + shadcn dialogs (ink theme, no
  `window.prompt`):
  - **Confirm** → `state='confirmed'`, joins the gallery.
  - **Reassign to another person** (search/select existing or create new) → repoint `person_id`,
    `state='confirmed'`, write a `person_reid_corrections` row.
  - **Mark Unknown** → `state='unknown'`, `person_id=NULL` (the low-confidence fallback bucket).
  - **Reject/Delete** → `state='rejected'` (or delete row + `media` object).
- `lib/data.ts` methods mirror the existing `enrollFace`/`listEmbeddings`/`approveEmbedding`
  block (`data.ts:188`): `listReidEmbeddings`, `reidPending`, `confirmReid`, `reassignReid`,
  `rejectReid`, `reidImageUrl`, `getReidConfig`/`saveReidConfig`.

## Phase 7 — Cluster visualization (spatial view of who-looks-like-whom)

A canvas view that projects the tenant's body embeddings to 2D so you can **see people cluster** —
tight groups = one person's samples; a `probably`/`unknown` crop drifting between two clusters is
exactly the ambiguous case the Review UI (Phase 6) resolves. It turns the abstract cosine space
into something spatial and eyeball-able.

- **Projection.** 512-d → 2D. Compute server-side in a control endpoint `GET
  /persons/reid/cluster` (RBAC `persons.read`, tenant-scoped): pull the tenant's embeddings, run
  **PCA/UMAP/t-SNE** → `[{embedding_id, person_id, person_name, state, x, y, image_key}]`. Start
  with **PCA** (cheap, deterministic, pure-Rust via `nalgebra`/`linfa`); upgrade to UMAP/t-SNE
  later if clusters overlap too much. Cache per-tenant (recompute on enroll or on demand — the set
  is small).
- **Canvas.** A pannable/zoomable 2D scatter reusing the **workflows editor canvas** pan/zoom
  idiom (`sentinel-app/src/routes/workflows/[[id]]/+page.svelte` — pan/zoom transform,
  world-cursor math), not Three.js (the Map's 3D stack is overkill for a 2D scatter). Each point
  is the crop thumbnail (tenant-checked `/persons/reid-embeddings/:eid/image`), tinted/outlined by
  `state`; points sharing a `person_id` share a hue (reuse the monochrome-ink accent treatment,
  not brand hues). Hover → enlarge + name/confidence; click → open that sample in the Review UI.
- **Payoff.** Spot a mislabeled sample sitting inside the wrong cluster, see when two people's
  wardrobes collide (clusters merge), and confirm the gallery is diverse (spread, not one dense
  blob). New route `sentinel-app/src/routes/people/clusters/+page.svelte`; data via a `lib/data.ts`
  `reidClusters()` method.
- **Scope guard:** read-only visualization in v1 (no drag-to-relabel on the canvas) — clicking
  routes to the existing Review actions. Drag-to-reassign is a nice v2.

## Phase 8 — Docs + deploy

- `docs/workflow-nodes.md`: add the **Recognize Body** + **Auto-Enroll Body** rows to the Vision
  section; note the `Detect Objects → Unpack Person → Recognize Body → Auto-Enroll Body` chain.
- `docs/ref/architecture.md`: note the second sidecar op (`reid_embed`) and that MinIO
  (`sentinel-media`) now also holds tenant-scoped ReID crops.
- New tables (`person_reid_embeddings`, `person_reid_config`, `person_reid_corrections`) auto-apply
  on control/flow boot (migration 0038). Ensure `S3_*` env is set wherever crops are written/read
  (control for enroll/review; flow if the AutoEnroll node writes) — same vars `media.rs` already uses.
- Build the sidecar image with OSNet baked in; deploy the sidecar (RTX 2060 / VPS CPU per
  [[ai-recognition-cloud-deploy]]). `task cargo` + `task check`;
  `task cloud-deploy -- sentinel-control sentinel-flow`, then frontend deploy.

## Phase 9 — v2: track-level identity (deferred)

Smooth per-frame labels into one stable identity per track — the `Track.uuid`
(`crates/app-core/src/tracking.rs:125`) is the per-session id; ReID maps it to a known person.
This is exactly the module's own TODO (`tracking.rs:12-14`, *"cross-session re-ID … later phase"*).
**Do not add a Track node** — extend the existing tracking events (standing rule).

Preferred wiring (privacy-preserving, edge-local embedding — route B):
1. **Edge computes the embedding** where the frame, `bbox`, and `uuid` already coexist — the
   confirmed-track loop in `sentinel-edge/src/tracking.rs` `run_track` (~lines 298–311). Crop
   `jpeg` to `t.bbox()`, run OSNet-via-`tract` (mirror `agent/detector.rs`), L2-normalize.
2. **Relay the 512-float vector, not the crop** — add an `embedding: Vec<f32>` field to
   `TrackEventOut` (`sentinel-edge/src/state.rs:335`) → `TrackEventIn`
   (`sentinel-control/src/http.rs:324`). (The motion path already ships `frame_b64`
   (`state.rs:372`), so imagery egress is possible if route A is chosen instead — but relaying
   only the vector keeps crops on-box.)
3. **Cloud matches + enriches the payload** — in `track_event_push` payload assembly
   (`sentinel-control/src/http.rs:355`), call `recognize_body` and add an `identity`/`person_id`
   field to the `object_entered`/`object_exited` payload **before** `event_queue::enqueue`
   (`http.rs:374`). It then surfaces automatically as an `OnEvent` output port via `payload_ports`
   (`domain.rs:188`) — graphs read `person`/`identity` with zero node changes.
4. **Majority-vote smoothing** across the track's frames keyed by `uuid`, so the label doesn't
   flicker per frame; assign on track close (`Exited`) or periodically.

Tracker jobs already derive from the `OnEvent("object_entered"/…)` nodes
(`sentinel-control/src/domain/workflows.rs:134`, config → `0037_tracking_config.sql` on
`tenant_boxes`) — extend that config path with a ReID on/off + model toggle.

---

## Open decisions (grill before building)

1. **Node naming.** `Recognize Body` (parallel to the face node "Recognize Person") vs.
   `Re-Identify Person` / `Identify by Appearance`. The face node owns "Recognize Person", so
   ReID needs a distinct label — recommend **Recognize Body**.
2. **Separate table vs. a `modality` column.** Recommend a **separate `person_reid_embeddings`**
   table (clean match queries, independent `model_version`, no accidental face↔body mixing) over
   adding `modality text` to `person_embeddings`. Confirm.
3. **Recognizer port shape.** Add `recognize_body` to the existing `Recognizer` trait
   (`ports.rs`) vs. a new `BodyRecognizer` port on `ToolContext`. Recommend one method on the
   existing port to avoid a second impl surface.
4. **Ensemble signals (v1 vs later).** Add a cheap clothing-color HSV histogram and/or bbox-height
   prior alongside the OSNet score, or ship embedding-only v1? Recommend **embedding-only v1**;
   revisit if outfit-change confusion shows up.
5. **Embedding runtime — DECIDED: run on the cloud AI sidecar (route A).** OSNet embedding is a
   new `reid_embed` op in `ai/sidecar/handler.py` (a `def reid_embed` + one `OPS` entry + a lazy
   `_reid_model()` loading OSNet ONNX beside InsightFace `buffalo_l`, reusing the GPU/CPU image
   split), called via `ai_service::call("reid_embed", json!({ "image_b64": image_b64(crop) }))` —
   mirroring `agent/face.rs::embed` exactly. Implication: the person **crop is sent to the cloud
   sidecar** (accept the egress; the motion path already ships `frame_b64`, so there's precedent).
   Still to confirm at build time: ONNX opset/provider (CUDA on the RTX 2060 vs. VPS CPU), 512-d
   output, and OSNet's RGB + ImageNet mean/std + 256×128 preprocessing on the Python side (differs
   from the edge YOLOX BGR/no-normalize, so it's sidecar-only prep — no `detector.rs` reuse).
   *(The edge-local `tract` alternative is parked as a possible v2 optimization for the track-level
   path if crop egress ever becomes a concern — not built now.)*
6. **Auto-enroll safety.** Auto-approval (`auto_enroll_enabled`) defaults **off** per tenant given
   clothing-driven false positives — high-confidence samples still get *captured* as `probably`,
   the toggle only controls whether they auto-join the gallery. Confirm this is the wanted default.
7. **RecognizeBody vs. AutoEnroll split.** AutoEnroll is a *separate* opt-in node (this expansion's
   ask) so graphs choose when to grow the gallery. Confirm you don't also want a combined
   "identify + auto-enroll" convenience node (recommend keeping them separate — composition is
   clearer and matches the single-responsibility node style).
8. **Crop retention + privacy.** Storing body crops in MinIO is new (the face feature kept vectors
   only). Decide: retention window / purge control, and whether crop storage is itself a per-tenant
   opt-in. Recommend opt-in + a "delete all crops" action, vectors kept.
9. **Cluster projection.** PCA (cheap, deterministic, pure-Rust) as the v1 2D projection, upgrading
   to UMAP/t-SNE only if clusters overlap. Confirm PCA-first, and whether the projection is
   recomputed on each enroll or on-demand when the view opens (recommend on-demand + short cache).

## Out of scope (v1)

- Cloth-changing / gait ReID (LTCC/CC-ReID) — overkill for a household wardrobe.
- Track-level smoothing + event enrichment (Phase 9).
- Cross-camera track hand-off / global track IDs.
- Vector quantization / ANN tuning (unneeded at gallery scale).
- Drag-to-relabel on the cluster canvas (Phase 7 is read-only in v1; use the Review actions).
- UMAP/t-SNE if PCA separates clusters well enough (start with PCA).

> **Note:** unlike the face feature (which deliberately kept vectors only), ReID **does** store the
> crop images in MinIO (tenant-scoped) — required by the Review UI + cluster view. This is a
> conscious privacy trade-off; gate crop retention behind the tenant's opt-in and allow purge.
