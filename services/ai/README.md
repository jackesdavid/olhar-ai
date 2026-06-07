# Sentinel AI sidecar

A small, portable vision service the desktop app calls over HTTP. One endpoint,
many **ops** (dispatched on `input.op`), so new capabilities are added without
touching the app's deploy. First op: **`detect_person`** (YOLOv8). The Sentinel
workflow **Person detection** node calls it.

The app talks to it exactly like it talks to Claude/ElevenLabs — via two env
vars in the repo‑root `.env`:

```
AI_SERVICE_URL=...   # RunPod runsync URL, or http://localhost:8000/runsync for dev
AI_SERVICE_KEY=...   # RunPod API key (omit/blank for local dev)
```

## Contract

```
POST <AI_SERVICE_URL>
Authorization: Bearer <AI_SERVICE_KEY>
{ "input": { "op": "detect_person", "image_b64": "<base64 jpeg>" } }

-> { "output": { "person": true, "count": 2, "confidence": 0.91 } }   # RunPod envelope
```

## Local development (free, no RunPod)

```bash
cd services/ai
python -m venv .venv && source .venv/bin/activate
pip install -r requirements.txt
python handler.py --rp_serve_api          # serves http://localhost:8000
```

Point the app at it:

```
AI_SERVICE_URL=http://localhost:8000/runsync
AI_SERVICE_KEY=
```

Smoke test:

```bash
IMG=$(base64 -i some.jpg | tr -d '\n')
curl -s localhost:8000/runsync -H 'content-type: application/json' \
  -d "{\"input\":{\"op\":\"detect_person\",\"image_b64\":\"$IMG\"}}" | jq .output
```

## Deploy to RunPod Serverless

1. **Build & push** the image (linux/amd64) to a registry (Docker Hub / GHCR).
   Set `AI_IMAGE` (in `.env` or inline), `docker login`, then:
   ```bash
   AI_IMAGE=<user>/sentinel-ai:latest task ai-deploy   # = ai-build + ai-push
   ```
   (Or the steps by hand: `task ai-build`, `task ai-push`.)
2. **RunPod → Serverless → New Endpoint**: set the container image to
   `<user>/sentinel-ai:latest`, pick the smallest GPU (or a CPU endpoint), leave
   workers **flex / scale‑to‑zero** (min 0). Container start command stays the
   image default (`python -u handler.py`).
3. Copy the **Endpoint ID** and a RunPod **API key**, then set in `.env`:
   ```
   AI_SERVICE_URL=https://api.runpod.ai/v2/<endpoint_id>/runsync
   AI_SERVICE_KEY=<runpod_api_key>
   ```

## Cost (flex / scale‑to‑zero)

Bills per second of execution; **$0 while idle**. A warm `detect_person` is
~0.3–1.5 s (≈ $0.0001–0.0008 on a low‑end GPU). A cold start adds ~5–20 s once
after idle. The workflow cascade (local **Motion** gate → `If` → Person → `If` →
Face) keeps paid calls tracking real events, so this is typically cents/month —
trigger snapshots on motion, not per frame. Keep a warm "active worker" only if
you need constant low latency (billed continuously). Verify live rates on the
RunPod pricing page.

## Adding an op

Write a function `my_op(inp: dict) -> dict`, register it in `OPS`, redeploy, then
add a Rust node that calls `agent::ai_service::call("my_op", json!({...}))`.
