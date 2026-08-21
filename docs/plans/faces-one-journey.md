# Faces — one journey per task, and delete means delete

**Board card:** #200 Faces: one journey per task, delete means delete, then re-test from zero
**Goal:** Collapse the duplicated face journeys into one, make deletion actually delete, wipe the
accumulated state, and re-walk each journey from a clean slate — testing one at a time.

## What is wrong today

**Two journeys write the same gallery.**

| Journey | Produced | State |
|---|---|---|
| Faces canvas → label → person | **103** embeddings | the real one |
| People → person → Enroll → upload | **1** embedding | redundant |
| People → person → Enroll → capture from camera | **0** | was broken until #195 |

**Deletion is a lie.** `faces::delete` runs `DELETE FROM faces … RETURNING image_key, crop_key` and
purges the MinIO objects. It never touches `person_embeddings`. Labeling copies the embedding into
the gallery (`label_faces`: `INSERT INTO person_embeddings … SELECT f.embedding … 'labeled'`), so
deleting the face removes the picture and **keeps the biometric template**.

That is the live 104-vs-3 gap: the canvas shows one known person; recognition knows six. The face
rows were hand-deleted after labeling and every embedding survived — unreachable from the canvas,
still steering recognition.

It is also a privacy problem. "Delete this face" has to mean it.

**Nothing records which face produced which embedding.** `person_embeddings` is
`(tenant_id, created_at, id, embedding, approved, approved_at, person_id, source)` — no `face_id`.
So a cascade cannot be written today; the link has to be added first.

**The only per-face delete lives on the screen we are retiring.** The person detail page lists
embeddings but can only delete the whole person; `deleteEmbedding` exists solely on
`/people/[id]/enroll`. Retiring that page without moving the grid would remove the only way to drop
a single bad face.

## Phases

### Phase 0 — wipe (destructive, needs an explicit go)
Production currently holds **7 persons, 104 embeddings, 3 faces** (3 with MinIO objects). Deleting
all of it is what makes the re-test meaningful: a gallery half-built by a broken journey cannot
validate the fixed one.

Wipe `person_embeddings`, `faces`, and their MinIO objects. **Keep `persons`** — the people are
real; only their biometrics are being rebuilt. Take a `pg_dump` of both tables first, so a wrong
call is recoverable.

### Phase 1 — delete means delete
Migration **0097** (next free number — `ls crates/app-core/migrations/` before writing it): add
`person_embeddings.face_id uuid NULL REFERENCES faces(id) ON DELETE CASCADE`. Nullable and
additive, so it is safe on a live DB. `label_faces` fills it. Deleting a face then removes its
embedding by the database's own rule rather than by a second query someone can forget.

Validate the SQL against a live database before shipping — a bad migration is a boot-retry loop.

### Phase 2 — one journey per task
- Move the face grid (view / approve / **delete a single face**) onto `/people/[id]`, where the
  gallery is otherwise invisible now that the raw crops are gone.
- Retire `/people/[id]/enroll` entirely — capture-from-camera and upload both.
- Keep `POST /persons/:id/embeddings`: labeling depends on it. Removing the *screen* is the point,
  not the endpoint.
- The canvas becomes the only way a face enters the gallery; the person page the only way one leaves.

### Phase 3 — re-walk each journey, one at a time
In order, each proven before the next starts:
1. **Capture** — phone Capture tab pushes; faces appear in the canvas as unknown.
2. **Label** — label one to a person; it joins the gallery (`source='labeled'`, `face_id` set).
3. **Recognise** — the same face recognised on a later capture, `found=true`, right person.
4. **Delete** — delete it in the canvas; confirm the embedding is gone and recognition stops.
5. **Enrol from the person page** — approve/delete still behave with no enrollment screen.

## Verification criteria

1. `person_embeddings` and `faces` are both empty after Phase 0; `persons` is untouched.
2. Deleting a labeled face leaves **zero** rows behind in `person_embeddings`.
3. `/people/[id]/enroll` returns 404 and nothing links to it.
4. A face can still be dropped from a person, on the person page.
5. Each Phase 3 step passes before the next is attempted.

## Out of scope
- #175 (phone camera as a listed camera source). Capture already works via the light edge.
- Changing thresholds or the scoring model — that is #123.
