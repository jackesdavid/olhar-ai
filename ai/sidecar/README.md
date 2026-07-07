# Sentinel AI sidecar

A small, portable **GPU** vision service the desktop app calls over HTTP. One
endpoint, many **ops** (dispatched on `input.op`), so new capabilities are added
without touching the app's deploy. Ops today:

- **`detect_person`** (YOLOv8) — used by the **Person detection** workflow node.
- **`face_detect_embed`** (InsightFace SCRFD + ArcFace) — detect the most prominent
  face **and** embed it in one call; used by **person enrollment** and **recognition**.
  Returns `face`, `box`, `kps`, `det_score`, and a 512-d L2-normalized `embedding`
  that matches the app's pgvector `vector(512)` column (cosine distance).
- **`face_detect`** (SCRFD only) — every face's `box` + 5-point `kps`, no embedding.
  Cheaper when you only need to locate/preview faces.
- **`face_embed`** (ArcFace only) — embed a face crop you already have. Pass `kps`
  (e.g. from `face_detect`) to align first for best accuracy; without `kps` the image
  is treated as an already-cropped face and resized to 112×112.

The three face ops share one `buffalo_l` pipeline; splitting them lets a caller skip
the half it doesn't need (detect without paying for ArcFace, or re-embed without
re-detecting).

The app talks to it exactly like it talks to Claude/ElevenLabs — via two env vars
in the repo-root `.env`:

```
AI_SERVICE_URL=...   # RunPod runsync URL, or http://localhost:8000/runsync for dev
AI_SERVICE_KEY=...   # RunPod API key (omit/blank for local dev)
```

## Contract

```
POST <AI_SERVICE_URL>
Authorization: Bearer <AI_SERVICE_KEY>
{ "input": { "op": "face_detect_embed", "image_b64": "<base64 jpeg>" } }

-> { "output": { "face": true, "count": 1, "box": [x1,y1,x2,y2], "kps": [[x,y]*5],
                 "embedding": [512 floats], "det_score": 0.94 } }   # RunPod envelope
```

Other ops:
- `face_detect`  -> `{ "face": true, "count": 2, "faces": [ { box, kps, det_score } ] }`
- `face_embed`   -> `{ "embedding": [512 floats], "dim": 512 }`  (input: `image_b64`, optional `kps`)
- `detect_person`-> `{ "person": true, "count": 2, "confidence": 0.91 }`

## Local development

`face_detect_embed` needs a CUDA GPU for full speed, but the same image runs on CPU
(InsightFace falls back automatically) — handy for a smoke test:

```bash
cd ai/sidecar
python -m venv .venv && source .venv/bin/activate
pip install -r requirements.txt          # swap onnxruntime-gpu -> onnxruntime for CPU-only
python handler.py --rp_serve_api          # serves http://localhost:8000
```

Point the app at it:

```
AI_SERVICE_URL=http://localhost:8000/runsync
AI_SERVICE_KEY=
```

Smoke test:

```bash
IMG=$(base64 -i face.jpg | tr -d '\n')
curl -s localhost:8000/runsync -H 'content-type: application/json' \
  -d "{\"input\":{\"op\":\"face_detect_embed\",\"image_b64\":\"$IMG\"}}" | jq '.output.face, (.output.embedding | length)'
```

## Run on a local NVIDIA box (LAN test — e.g. an RTX 2060)

The GPU image runs unchanged on any NVIDIA card; for a LAN test you just launch the
container with the GPU passed through and start RunPod's built-in local server
(same `/runsync` contract). On the Linux box (Docker + NVIDIA driver +
nvidia-container-toolkit, so `nvidia-smi` works on the host):

```bash
cd ai/sidecar
docker compose -f compose.gpu.yml up --build      # builds natively + runs on the GPU
```

Point the Mac app at the box in the repo-root `.env`:

```
AI_SERVICE_URL=http://<linux-box-lan-ip>:8000/runsync
AI_SERVICE_KEY=
```

Smoke test from the Mac (or the box):

```bash
IMG=$(base64 -i face.jpg | tr -d '\n')
curl -s http://<linux-box-lan-ip>:8000/runsync -H 'content-type: application/json' \
  -d "{\"input\":{\"op\":\"face_detect_embed\",\"image_b64\":\"$IMG\"}}" \
  | jq '.output.face, (.output.embedding | length)'
```

Then enroll a face in the app — it now embeds on the box's GPU. (To pull the
CI-built image instead of building on the box, edit `compose.gpu.yml` per its
comments and `docker login ghcr.io` first.)

## Deploy to RunPod Serverless (GPU)

1. **Build & push** the image (linux/amd64) to GHCR. Set `AI_IMAGE`, `docker
   login ghcr.io`, then:
   ```bash
   AI_IMAGE=ghcr.io/jackesdavid/sentinel-ai:gpu task ai-deploy   # = ai-build + ai-push
   ```
2. **RunPod → Serverless → New Endpoint**: container image
   `ghcr.io/jackesdavid/sentinel-ai:gpu`, pick a **GPU** worker — the cloud endpoint
   is set to run on an **RTX A4500** (20 GB; a 16 GB card is otherwise plenty),
   workers **flex / scale-to-zero** (min 0). Start command stays the image default
   (`python -u handler.py`).
3. Copy the **Endpoint ID** and a RunPod **API key** into `.env`:
   ```
   AI_SERVICE_URL=https://api.runpod.ai/v2/<endpoint_id>/runsync
   AI_SERVICE_KEY=<runpod_api_key>
   ```

The image is large (CUDA torch + onnxruntime-gpu + InsightFace weights). The first
cold start downloads/initialises the GPU pipeline (~10–30 s once); warm
`face_detect_embed` calls are tens of milliseconds.

> **CUDA / onnxruntime pairing** is the one fragile knob: the Dockerfile base CUDA
> major and the `onnxruntime-gpu` pin in `requirements.txt` must match. If a build
> or first call errors on CUDA, adjust those two together.

## Cost (flex / scale-to-zero)

Bills per second of execution; **$0 while idle**. A warm call is sub-second; a cold
start adds a one-time GPU init. Enrollment is user-triggered (a handful of calls),
so this is typically cents. Keep a warm worker only if you need constant low
latency (billed continuously).

## Adding an op

Write a function `my_op(inp: dict) -> dict`, register it in `OPS`, redeploy, then
call it from Rust via `agent::ai_service::call("my_op", json!({...}))`.
