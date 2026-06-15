"""Sentinel AI sidecar — a portable, op-dispatched vision service.

Runs on RunPod Serverless (one endpoint, many ops) and locally for dev via
`python handler.py --rp_serve_api` (same `/runsync` contract on localhost).

Request  (RunPod envelope):  { "input": { "op": "<name>", ...args } }
Response (RunPod envelope):  { "output": { ...result } }

Add a new capability by writing a function and registering it in OPS — the Rust
app calls it by name through `agent::ai_service`. Ops:
  • `detect_person`     — YOLOv8-Nano person presence (COCO class 0).
  • `face_detect_embed` — SCRFD detect + ArcFace 512-d embed of the largest face.
  • `face_detect`       — SCRFD only: every face's bbox + 5-point landmarks.
  • `face_embed`        — ArcFace only: embed a face crop (aligned via `kps` if given).

The three face ops share one InsightFace `buffalo_l` pipeline (SCRFD detector +
ArcFace recognizer); splitting them lets callers skip the half they don't need.
"""

import base64
import io
import time

import cv2
import numpy as np
import runpod
from PIL import Image

# COCO class 0 == "person".
_PERSON_CLASS = 0

# Models are lazily initialised on first use. The face pipeline (InsightFace) and the
# YOLO person detector load independently so a **face-only** deployment — the CPU
# sidecar on the VPS, which omits torch/ultralytics to stay small — never needs YOLO.
_FACE_APP = None
_MODEL = None


def _yolo():
    """YOLOv8-Nano person detector, lazily imported. Face-only images (no torch/
    ultralytics) never call this; if `detect_person` is invoked there, raise a clear
    error instead of failing at import time. (The Rust app detects people locally via
    tract, so the cloud path never hits this op.)"""
    global _MODEL
    if _MODEL is None:
        try:
            from ultralytics import YOLO
        except ImportError as e:
            raise RuntimeError(
                "detect_person needs ultralytics+torch, which this face-only image omits"
            ) from e
        _MODEL = YOLO("yolov8n.pt")
    return _MODEL


def _decode_bgr(inp: dict) -> np.ndarray:
    """Decode `image_b64` to a contiguous BGR array (OpenCV/InsightFace convention)."""
    b64 = inp.get("image_b64") or ""
    if not b64:
        raise ValueError("missing 'image_b64'")
    raw = base64.b64decode(b64)
    rgb = Image.open(io.BytesIO(raw)).convert("RGB")
    # PIL is RGB; InsightFace wants BGR. ascontiguous so OpenCV/ORT see positive strides.
    return np.ascontiguousarray(np.asarray(rgb)[:, :, ::-1])


def _face_app():
    """InsightFace `buffalo_l` — SCRFD detector + ArcFace w600k_r50 embedder.

    Only the detection + recognition models are loaded (genderage / landmark models
    in the pack are skipped). `ctx_id` selects the device: a CUDA device (0) when
    onnxruntime exposes the CUDA provider (GPU image), else -1 for CPU (the VPS
    sidecar). Exposes the sub-models via `.models['detection']` / `.models['recognition']`
    so the detect-only and embed-only ops can run just their half.
    """
    global _FACE_APP
    if _FACE_APP is None:
        import insightface
        import onnxruntime as ort

        ctx_id = 0 if "CUDAExecutionProvider" in ort.get_available_providers() else -1
        print(f"[ai] InsightFace ctx_id={ctx_id} ({'GPU' if ctx_id >= 0 else 'CPU'})", flush=True)
        app = insightface.app.FaceAnalysis(
            name="buffalo_l", allowed_modules=["detection", "recognition"]
        )
        app.prepare(ctx_id=ctx_id, det_size=(640, 640))
        _FACE_APP = app
    return _FACE_APP


def _box(bbox) -> list:
    return [float(v) for v in (bbox.tolist() if hasattr(bbox, "tolist") else bbox)]


def _kps(kps) -> list:
    if kps is None:
        return []
    pts = kps.tolist() if hasattr(kps, "tolist") else kps
    return [[float(x), float(y)] for x, y in pts]


def detect_person(inp: dict) -> dict:
    """Detect people in an image. -> { person, count, confidence }."""
    img = Image.fromarray(_decode_bgr(inp)[:, :, ::-1])  # YOLO takes RGB
    results = _yolo().predict(img, classes=[_PERSON_CLASS], verbose=False)
    boxes = results[0].boxes
    count = int(len(boxes))
    confidence = float(boxes.conf.max()) if count else 0.0
    return {"person": count > 0, "count": count, "confidence": round(confidence, 3)}


def face_detect_embed(inp: dict) -> dict:
    """Detect the most prominent face AND embed it (the enrollment path).

    -> { face: bool, count: int, box: [x1,y1,x2,y2], kps: [[x,y]*5],
         embedding: [512 floats, L2-normalized], det_score: float }. `embedding`/`box`
    are empty and `face` is false when no face is found. The vector matches the app's
    pgvector `vector(512)` column and is compared with cosine distance.
    """
    bgr = _decode_bgr(inp)
    faces = _face_app().get(bgr)
    if not faces:
        return {"face": False, "count": 0, "box": [], "kps": [], "embedding": [], "det_score": 0.0}

    # Pick the largest face by bounding-box area (the subject being enrolled).
    f = max(faces, key=lambda x: (x.bbox[2] - x.bbox[0]) * (x.bbox[3] - x.bbox[1]))
    return {
        "face": True,
        "count": len(faces),
        "box": _box(f.bbox),
        "kps": _kps(f.kps),
        "embedding": [float(v) for v in f.normed_embedding.tolist()],
        "det_score": float(f.det_score),
    }


def face_detect(inp: dict) -> dict:
    """Detect faces only — no embedding (cheaper than the full pipeline).

    -> { face: bool, count: int, faces: [ { box, kps, det_score } ] }, sorted by
    detector score (highest first). `kps` are the 5 landmarks you can feed back into
    `face_embed` to align before extracting.
    """
    bgr = _decode_bgr(inp)
    det = _face_app().models["detection"]
    bboxes, kpss = det.detect(bgr, max_num=0, metric="default")
    faces = []
    for i in range(bboxes.shape[0]):
        x1, y1, x2, y2, score = bboxes[i].tolist()
        faces.append(
            {
                "box": [float(x1), float(y1), float(x2), float(y2)],
                "kps": _kps(kpss[i]) if kpss is not None else [],
                "det_score": float(score),
            }
        )
    faces.sort(key=lambda f: f["det_score"], reverse=True)
    return {"face": len(faces) > 0, "count": len(faces), "faces": faces}


def face_embed(inp: dict) -> dict:
    """Embed a face crop only — no detection (ArcFace, 512-d, L2-normalized).

    Pass `kps` (5 landmarks, e.g. from `face_detect`) to align with the same
    similarity transform the full pipeline uses — best accuracy. Without `kps` the
    image is assumed to already be a tight, roughly-centred face crop and is just
    resized to 112×112. -> { embedding: [512 floats], dim: int }.
    """
    bgr = _decode_bgr(inp)
    rec = _face_app().models["recognition"]
    kps = inp.get("kps")
    if kps:
        from insightface.utils import face_align

        aligned = face_align.norm_crop(bgr, landmark=np.array(kps, dtype=np.float32), image_size=112)
    else:
        aligned = cv2.resize(bgr, (112, 112))

    feat = np.asarray(rec.get_feat([aligned])[0], dtype=np.float32).flatten()
    norm = float(np.linalg.norm(feat))
    normed = feat / norm if norm > 0 else feat
    return {"embedding": [float(v) for v in normed.tolist()], "dim": int(normed.shape[0])}


# op name -> handler function. Extend this for future capabilities
# (e.g. "detect_objects", "read_plate", ...).
OPS = {
    "detect_person": detect_person,
    "face_detect_embed": face_detect_embed,
    "face_detect": face_detect,
    "face_embed": face_embed,
}


def handler(job: dict) -> dict:
    inp = job.get("input") or {}
    op = inp.get("op")
    fn = OPS.get(op)
    if fn is None:
        return {"error": f"unknown op '{op}'. known: {sorted(OPS)}"}
    # Time the op (server-side, GPU + decode only — the first face call also pays the
    # one-time model load). Compare against the app's wall-clock log to split GPU time
    # from network / cold-start. `took_ms` is echoed in the response too.
    t0 = time.perf_counter()
    try:
        out = fn(inp)
        ms = round((time.perf_counter() - t0) * 1000)
        print(f"[ai] op={op} took {ms} ms", flush=True)
        if isinstance(out, dict):
            out.setdefault("took_ms", ms)
        return out
    except Exception as e:  # surface a clean error to the caller
        ms = round((time.perf_counter() - t0) * 1000)
        print(f"[ai] op={op} FAILED in {ms} ms: {e}", flush=True)
        return {"error": f"{op}: {e}"}


if __name__ == "__main__":
    runpod.serverless.start({"handler": handler})
