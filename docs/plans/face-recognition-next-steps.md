# Face Recognition — State & Next Steps

**Board card:** Face recognition next steps (ToDo)
**Goal:** Complete the workflow node family, enable live-feed auto-recognition, and upgrade the AI sidecar to GPU.

---

## Current state (as of 2026-06-18)

### What is fully working
- **Enrollment flow** — capture from camera or upload → `face_detect_embed` → 512-d ArcFace vector stored in `person_embeddings` (pgvector) → approve per embedding
- **Manual recognition page** (`/people/recognize`) — probe image → embed → pgvector kNN → best cosine match with confidence score
- **`RecognizePerson` workflow node** — takes an `Image` port, calls sidecar, outputs `person`/`person_id`/`confidence`/`ok`
- **`BreakFaceDetection` node** — decomposes a `Face` port into bbox/confidence/landmarks
- **AI sidecar** (`ai/sidecar/`, InsightFace `buffalo_l`) — three ops exposed: `face_detect_embed`, `face_detect`, `face_embed`
- **Cloud CPU sidecar** — live on VPS since 2026-06-15; no RunPod cold-start for enrolled users
- **Multi-tenant isolation** — `(tenant_id, person_id)` composite keys throughout

### What is missing
| Gap | Impact |
|-----|--------|
| No `DetectFaces` workflow node | `BreakFaceDetection` has no producer; `face_detect` op is unreachable from the graph |
| No live-feed auto-recognition | Recognition only happens on the manual `/people/recognize` page; no workflow trigger |
| RTX 2060 box not yet the GPU sidecar | All inference runs CPU on VPS (slower, no CUDA speedup) |
| No batch re-enrollment | If model updates (e.g., buffalo_s → buffalo_l+), existing vectors can't be refreshed |
| No model version field on embeddings | Vectors from different model versions would silently mix |

---

## Phase 1 — `DetectFaces` workflow node (quick win)

Wrap the `face_detect` sidecar op as a node so graphs can produce `FaceArray` without embedding.

**Steps:**
1. Add `NodeKind::DetectFaces` to `crates/app-core/src/workflows/node.rs`
   - Input: `image` (Image)
   - Output: `faces` (FaceArray), `count` (Num), `any` (Bool)
   - Config: none (threshold lives in sidecar model)
2. Implement `run_detect_faces` — call `ai_service::call("face_detect", …)`, deserialize into `Vec<FaceDetection>`, emit as `FaceArray` port
3. Add to `NodeKind::ALL` (catalog auto-updates; no changes to `agent/tools.rs`)
4. Front-end: node card appears automatically from catalog; verify in workflow editor

This completes the detection family: `DetectFaces → BreakFaceDetection → RecognizePerson` is now a valid chain.

---

## Phase 2 — Live-feed auto-recognition trigger

Enable a workflow to auto-recognize faces from camera frames, e.g. "when a face is detected, recognize and record presence."

**Pattern:**
```
CameraFrame → DetectFaces → ForEach(face) → BreakFaceDetection → RecognizePerson → RecordPresence
```

**Steps:**
1. Verify `CameraFrame` (or `Snapshot`) node emits `Image` — confirm port compatibility with `DetectFaces`
2. Test the chain above end-to-end in workflow editor; fix any port-type mismatches
3. Measure round-trip latency on CPU sidecar (target: <500 ms per frame for usable live recognition)
4. Add a `frame_interval_secs` config on the source node or a `Throttle` node to avoid spamming the sidecar

No new backend work needed if Phase 1 is done; this is integration + testing.

---

## Phase 3 — RTX 2060 GPU sidecar upgrade

Move inference from CPU-on-VPS to the LAN RTX 2060 box over the tailnet.

**Steps:**
1. On the RTX 2060 Linux box: install NVIDIA drivers, `nvidia-container-toolkit`, Docker, Tailscale (join the tailnet)
2. Run `docker compose -f ai/sidecar/compose.gpu.yml up -d` — confirm `face_detect_embed` responds on port 8000
3. In the VPS `/ibbf` compose, update `AI_SERVICE_URL` in `sentinel.env` to `http://<box-tailnet-ip>:8000/runsync`
4. Add `extra_hosts` entry for the box in the sentinel-control service block (same pattern as the gateway → edge)
5. Keep the VPS CPU sidecar as a hot standby (consider `AI_SERVICE_FALLBACK_URL` env var in `ai_service.rs`)
6. Measure latency improvement (expect 10–30× speedup for face ops vs CPU)

---

## Phase 4 — Operational hardening (deferred)

- **Batch re-enrollment** — `/persons/embeddings/reembed` endpoint that iterates all stored persons, fetches their enrollment photos (or prompts user to re-upload), and replaces vectors
- **Model version tracking** — add `model_version text` column to `person_embeddings` (migration 0023); stamp on insert; warn on recognize when versions differ
- **Threshold calibration** — expose per-tenant `max_distance` default in Settings; log false-positive pairs for hard-negative mining

---

## Open decisions

1. **Fallback strategy** — should `AI_SERVICE_FALLBACK_URL` be a first-class env var in `ai_service.rs`, or just an ops-level redirect (HAProxy/Nginx in front of sidecar)? The former is more explicit; the latter is invisible to the app.
2. **Frame source for live recognition** — use the existing `Snapshot` node (JPEG, ~1/s) or add a `CameraFrame` node that taps go2rtc at a configurable interval? The `Snapshot` node is already wired and simpler.
3. **Attendance integration** — should `RecognizePerson → RecordPresence` happen via a user-built workflow (flexible) or a first-class "Auto-Attendance" node that bundles both (simpler for non-technical users)?

---

## Out of scope (next phases)

- Vector quantization (8-bit pgvector) — not needed until embedding count is in the thousands
- Multi-model ensemble (buffalo_l + dlib) — the A/B bench showed no accuracy gain worth the complexity
- Photo storage (cropped face thumbnails) — privacy-sensitive; deliberate omission (only vectors kept)
