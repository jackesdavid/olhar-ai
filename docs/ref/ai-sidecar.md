# AI Sidecar — Reference

The GPU/CPU Python vision service that powers face recognition and person detection.

## Overview

A RunPod-compatible Python service (`ai/sidecar/handler.py`) dispatched by op name via POST
`/runsync`. Three deployment targets: RunPod Serverless (prod fallback), VPS CPU container
(current cloud), and a local GPU box (in-progress). The Rust app calls it through
`crates/app-core/src/agent/ai_service.rs` using `AI_SERVICE_URL`.

## File layout

```
ai/sidecar/
├── handler.py            — main op dispatcher (detect_person, face_detect_embed, face_detect, face_embed)
├── Dockerfile            — GPU image: pytorch:2.4.1-cuda12.4-cudnn9-runtime base + insightface
├── Dockerfile.cpu        — CPU image: face-only, no torch/ultralytics, smaller footprint
├── compose.gpu.yml       — local GPU box: podman/docker run on port 8000
├── requirements.txt      — GPU deps: ultralytics, insightface, onnxruntime-gpu>=1.17
├── requirements.cpu.txt  — CPU deps: insightface, onnxruntime (CPU), no torch
```

## Ops exposed

| Op | Input | Output | Used by |
|----|-------|--------|---------|
| `face_detect_embed` | `image_b64` | `{face, count, box, kps, embedding[512], det_score}` | Enrollment + RecognizePerson node |
| `face_detect` | `image_b64` | `{face, count, faces[{box,kps,det_score}]}` | (DetectFaces node — pending) |
| `face_embed` | `image_b64`, optional `kps` | `{embedding[512]}` | Downstream crop embedding |
| `detect_person` | `image_b64` | `{person, count, confidence}` | Unused in cloud (tract handles it locally) |

## Models

- **buffalo_l** (InsightFace): SCRFD detector + ArcFace w600k_r50 embedder, 512-d L2-normalized vectors
- **yolov8n.pt**: COCO class-0 person detector (GPU image only; CPU image omits)
- Models are baked into the image at build time (no cold-start download)

## Deployments

| Target | Image | URL | Notes |
|--------|-------|-----|-------|
| VPS CPU | `sentinel-ai:cpu` | `http://sentinel-ai:8000/runsync` | Internal service in `/ibbf` compose; no Traefik |
| GPU box (blocked) | `sentinel-ai:gpu` | `http://100.84.250.126:8000/runsync` | See CUDA mismatch below |
| RunPod Serverless | `sentinel-ai:gpu` | RunPod URL + key | Fallback; cold-start ~2 min |
| Dev (Mac) | CPU sidecar | `http://localhost:8000/runsync` | `task dev` starts it, overrides .env |

## GPU box (RTX 2060 — in-progress)

- **Machine:** Bazzite Linux, hostname `customer`, user `jack`
- **Tailnet IP:** `100.84.250.126` — SSH alias `gpu-box` in `~/.ssh/config` with `~/.ssh/id_rsa`
- **NOPASSWD sudo:** `/usr/bin/podman` only (`/etc/sudoers.d/sentinel-ai`)
- **Driver:** 595.71.05, CUDA 13.2 capable; RTX 2060 SUPER 8 GB
- **CDI config:** `/etc/cdi/nvidia.yaml` present; `--device nvidia.com/gpu=all` works with `sudo podman`
- **Image:** `localhost/sentinel-ai:gpu` — built (8.4 GB), container `sentinel-ai` running but on CPU

### CUDA version mismatch (active blocker)

The Dockerfile base is `pytorch:2.4.1-cuda12.4-cudnn9-runtime` (ships `libcudart.so.12`).
PyPI onnxruntime-gpu wheels have a gap: CUDA 11 (≤1.17.x) and CUDA 13 (≥1.19.x) — no CUDA 12
wheel on the default index. CUDA 12.4 apt packages installed in the running container but
haven't resolved the version gap.

| onnxruntime-gpu version | libcudart needed | Status |
|-------------------------|-----------------|--------|
| 1.27.0 | `.so.13` | ✗ no CUDA 13 in image |
| 1.19.2 | `.so.13` | ✗ same |
| **1.18.x** | TBD | **Next to test** — likely CUDA 12 sweet spot |
| 1.17.1 | `.so.11.0` | ✗ no CUDA 11 in image |
| 1.16.3 | `.so.11.0` | ✗ same |

**Next step:** try `onnxruntime-gpu==1.18.1` in the container. If it needs `.so.12`, combine with the
already-installed `cuda-cudart-12-4` apt libs and update `LD_LIBRARY_PATH`:
```
/usr/local/cuda-12/lib64:/opt/conda/lib/python3.11/site-packages/nvidia/cublas/lib:...
```

**Fallback:** download the CUDA 12 wheel directly from the ONNX Runtime GitHub releases page
(e.g., `onnxruntime_gpu-1.19.2-cp311-cp311-linux_x86_64-cuda12.whl`) instead of using PyPI.

### Deploy commands (once CUDA is working)

```bash
# Rsync source (Dockerfile.gpu has docker.io/ prefix for Podman short-name policy)
rsync -av ai/sidecar/ gpu-box:~/sentinel-ai/

# Build
ssh gpu-box "nohup sudo podman build -t sentinel-ai:gpu \
  -f ~/sentinel-ai/Dockerfile.gpu ~/sentinel-ai/ > /tmp/sidecar-build.log 2>&1 &"

# Run with GPU
ssh gpu-box "sudo podman run -d --device nvidia.com/gpu=all -p 8000:8000 \
  --name sentinel-ai --restart unless-stopped sentinel-ai:gpu \
  python -u handler.py --rp_serve_api --rp_api_host 0.0.0.0 --rp_api_port 8000"

# Smoke test from Mac
curl -s -X POST http://100.84.250.126:8000/runsync \
  -H "Content-Type: application/json" \
  -d '{"input":{"op":"face_detect","image_b64":""}}' | jq .
```

## Build status

| Item | Status |
|------|--------|
| CPU sidecar on VPS | ✅ Live — serving enrollment + recognition in prod |
| `sentinel-ai:gpu` image on gpu-box | ✅ Built (8.4 GB) |
| CUDAExecutionProvider in container | ❌ Blocked — CUDA version mismatch (try 1.18.x next) |
| GPU box wired into VPS control (`AI_SERVICE_URL`) | ❌ Pending CUDA fix |
| DetectFaces workflow node | ❌ Not started (Phase 1 — `docs/plans/face-recognition-next-steps.md`) |
| Live-feed auto-recognition | ❌ Not started (Phase 2) |
