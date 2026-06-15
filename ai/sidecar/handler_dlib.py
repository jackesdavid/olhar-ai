"""Sentinel AI sidecar — dlib / `face_recognition` variant (A/B experiment).

Same op-dispatch + `/runsync` contract as handler.py (InsightFace), so it's a drop-in
swap: point AI_SERVICE_URL (or bench.py --url) at this service to compare. Uses the
dlib-based `face_recognition` library (HOG or CNN detector + dlib ResNet embedding).

IMPORTANT: embeddings here are **128-d** vs InsightFace's **512-d** — they are NOT
comparable, and the app's pgvector column is `vector(512)`. This sidecar is for
detection / latency / accuracy A/B testing via bench.py, not a DB-compatible swap.

Ops (mirroring handler.py, minus detect_person — `face_recognition` does no person
detection; the Rust app handles that locally via tract):
  • face_detect_embed — detect the largest face + embed it (the enrollment path).
  • face_detect       — detect faces only (bboxes).
  • face_embed        — embed (detects then encodes the first face).
Each op accepts an optional `model`: "hog" (default, fast CPU) or "cnn" (dlib's
accurate CNN detector — much slower on CPU, benefits from a GPU).
"""
import base64
import io
import time

import numpy as np
import runpod
from PIL import Image
import face_recognition


def _decode_rgb(inp: dict) -> np.ndarray:
    """Decode `image_b64` to a contiguous RGB array (face_recognition's convention)."""
    b64 = inp.get("image_b64") or ""
    if not b64:
        raise ValueError("missing 'image_b64'")
    raw = base64.b64decode(b64)
    return np.ascontiguousarray(np.asarray(Image.open(io.BytesIO(raw)).convert("RGB")))


def _model(inp: dict) -> str:
    return "cnn" if str(inp.get("model", "")).lower() == "cnn" else "hog"


def _area(loc) -> int:
    # face_recognition locations are (top, right, bottom, left).
    t, r, b, l = loc
    return (r - l) * (b - t)


def face_detect_embed(inp: dict) -> dict:
    """Detect the most prominent face AND embed it. -> { face, count, box:[x1,y1,x2,y2],
    embedding:[128 floats], model }."""
    rgb = _decode_rgb(inp)
    model = _model(inp)
    locs = face_recognition.face_locations(rgb, model=model)
    if not locs:
        return {"face": False, "count": 0, "box": [], "embedding": [], "model": model}
    best = max(locs, key=_area)
    t, r, b, l = best
    enc = face_recognition.face_encodings(rgb, known_face_locations=[best])[0]
    return {
        "face": True,
        "count": len(locs),
        "box": [float(l), float(t), float(r), float(b)],
        "embedding": [float(v) for v in enc.tolist()],
        "model": model,
    }


def face_detect(inp: dict) -> dict:
    """Detect faces only. -> { face, count, faces:[{box}], model }, largest first."""
    rgb = _decode_rgb(inp)
    model = _model(inp)
    locs = sorted(face_recognition.face_locations(rgb, model=model), key=_area, reverse=True)
    faces = [{"box": [float(l), float(t), float(r), float(b)]} for (t, r, b, l) in locs]
    return {"face": len(faces) > 0, "count": len(faces), "faces": faces, "model": model}


def face_embed(inp: dict) -> dict:
    """Embed the first detected face. -> { embedding:[128 floats], dim }."""
    rgb = _decode_rgb(inp)
    encs = face_recognition.face_encodings(rgb)
    if not encs:
        return {"embedding": [], "dim": 0}
    e = encs[0]
    return {"embedding": [float(v) for v in e.tolist()], "dim": int(len(e))}


OPS = {
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
    t0 = time.perf_counter()
    try:
        out = fn(inp)
        ms = round((time.perf_counter() - t0) * 1000)
        print(f"[dlib] op={op} took {ms} ms", flush=True)
        if isinstance(out, dict):
            out.setdefault("took_ms", ms)
        return out
    except Exception as e:
        ms = round((time.perf_counter() - t0) * 1000)
        print(f"[dlib] op={op} FAILED in {ms} ms: {e}", flush=True)
        return {"error": f"{op}: {e}"}


if __name__ == "__main__":
    # face_recognition loads its detector/predictor/encoder models at import; touch the
    # HOG path once so the first real request is warm (matches handler.py's startup warmup).
    try:
        t0 = time.perf_counter()
        face_recognition.face_locations(np.zeros((128, 128, 3), dtype=np.uint8))
        print(f"[dlib] warmed at startup in {round((time.perf_counter()-t0)*1000)} ms", flush=True)
    except Exception as e:
        print(f"[dlib] startup warmup note: {e}", flush=True)
    runpod.serverless.start({"handler": handler})
