"""Sentinel AI sidecar — a portable, op-dispatched vision service.

Runs on RunPod Serverless (one endpoint, many ops) and locally for dev via
`python handler.py --rp_serve_api` (same `/runsync` contract on localhost).

Request  (RunPod envelope):  { "input": { "op": "<name>", ...args } }
Response (RunPod envelope):  { "output": { ...result } }

Add a new capability by writing a function and registering it in OPS — the Rust
app calls it by name through `agent::ai_service`. The first op is `detect_person`.
"""

import base64
import io

import runpod
from PIL import Image
from ultralytics import YOLO

# Loaded once per worker (cached in the image at build time — see Dockerfile).
_MODEL = YOLO("yolov8n.pt")

# COCO class 0 == "person".
_PERSON_CLASS = 0


def _decode_image(inp: dict) -> Image.Image:
    b64 = inp.get("image_b64") or ""
    if not b64:
        raise ValueError("missing 'image_b64'")
    raw = base64.b64decode(b64)
    return Image.open(io.BytesIO(raw)).convert("RGB")


def detect_person(inp: dict) -> dict:
    """Detect people in an image. -> { person, count, confidence }."""
    img = _decode_image(inp)
    results = _MODEL.predict(img, classes=[_PERSON_CLASS], verbose=False)
    boxes = results[0].boxes
    count = int(len(boxes))
    confidence = float(boxes.conf.max()) if count else 0.0
    return {"person": count > 0, "count": count, "confidence": round(confidence, 3)}


# op name -> handler function. Extend this for future capabilities
# (e.g. "face_embedding", "detect_objects", "read_plate", ...).
OPS = {
    "detect_person": detect_person,
}


def handler(job: dict) -> dict:
    inp = job.get("input") or {}
    op = inp.get("op")
    fn = OPS.get(op)
    if fn is None:
        return {"error": f"unknown op '{op}'. known: {sorted(OPS)}"}
    try:
        return fn(inp)
    except Exception as e:  # surface a clean error to the caller
        return {"error": f"{op}: {e}"}


if __name__ == "__main__":
    runpod.serverless.start({"handler": handler})
