# Sidecar test fixtures — stored face frames

Real frames captured live from the edge and stored by the `StoreFace` workflow node, kept as
fixtures so we can **A/B a local sidecar against the cloud (RunPod GPU) reference** on the exact
same input — same frame, same bbox, same crop geometry production used.

## `face_frame_01`
- **`face_frame_01.jpg`** — the full 640×360 frame (17 KB). A single face at bbox `[290,71,355,171]`
  (65×100 px), detected by the edge's YuNet and delivered via the `face_detected` event.
- **`face_frame_01.embedding.json`** — the 512-d L2-normalized ArcFace embedding the **cloud**
  sidecar (`face_detect_embed`, RunPod GPU, `buffalo_l`) produced for this frame in run
  `7244b346…`. This is the reference to compare against.
- **`face_frame_01.json`** — metadata: bbox, frame size, the 30% crop margin + resulting crop
  region, provider, and the source face id.

This face landed as *unknown* (no match), which is why it's in the Faces labeling queue.

## Compare a local sidecar

```sh
cd ai/sidecar          # so `import handler` + model weights resolve
python testdata/compare_local.py
```

`compare_local.py` reproduces production exactly — crops the frame to the bbox with the same 30%
margin as `crop_to_bbox` (`crates/app-core/src/workflows/node/vision.rs`), then runs the local
`handler.face_detect_embed` on the crop. If the local SCRFD fails to re-detect (the intermittent
"No face found" this fixture was captured to study), it falls back to `handler.face_embed`
(align-free) — mirroring the `embed_face_crop` fix in `crates/app-core/src/agent/face.rs`. For
each path it reports **detected? · wall time · cosine similarity to the cloud reference**.

A local sidecar matching the cloud should give **cos ≈ 1.0**; the same identity across providers
should stay well under the 0.40 recognition distance (**cos > 0.60**).

## Adding more fixtures
Grab any stored face from the `faces` table + MinIO:
1. `SELECT image_key, bbox, array_to_json(embedding::float4[]) FROM faces WHERE id='…';`
2. `mc cat m/sentinel-media/<image_key>` → `face_frame_NN.jpg`
3. Write `face_frame_NN.json` (copy the shape above) + `face_frame_NN.embedding.json`.
4. `python testdata/compare_local.py testdata/face_frame_NN.json`
