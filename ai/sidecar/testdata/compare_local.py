#!/usr/bin/env python3
"""A/B a LOCAL sidecar against the CLOUD reference embedding for a stored face frame.

Reproduces production exactly: loads the stored frame, crops to the edge's bbox with the
same 30% margin that `crop_to_bbox` (crates/app-core/src/workflows/node/vision.rs) applies,
then runs the local sidecar's `face_detect_embed` on the crop. If SCRFD fails to re-detect
(the intermittent "No face found" this fixture was captured to study), it falls back to the
align-free `face_embed` — mirroring the `embed_face_crop` fix. Reports, for each path:
detected?, wall time, and cosine similarity to the RunPod reference embedding.

Run from the sidecar dir so `import handler` resolves (needs its deps / model weights):
    cd ai/sidecar && python testdata/compare_local.py
Optionally point at a different fixture: python testdata/compare_local.py testdata/face_frame_02.json
"""
import base64
import io
import json
import math
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent))  # so `import handler` finds ai/sidecar/handler.py

import handler  # noqa: E402  (the local sidecar under test)


def _b64(path: Path) -> str:
    return base64.b64encode(path.read_bytes()).decode()


def _crop_b64(frame_path: Path, bbox, margin_pct: int) -> str:
    """Grow bbox by margin_pct on each side, clamp to the frame, return the JPEG crop as b64.
    Integer math mirrors crop_to_bbox so the crop matches production byte-for-byte in geometry."""
    from PIL import Image

    img = Image.open(frame_path).convert("RGB")
    iw, ih = img.size
    x1, y1, x2, y2 = bbox
    bw, by = max(x2 - x1, 1), max(y2 - y1, 1)
    mx, my = bw * margin_pct // 100, by * margin_pct // 100
    cx1 = min(max(x1 - mx, 0), iw - 1)
    cy1 = min(max(y1 - my, 0), ih - 1)
    cx2 = min(max(x2 + mx, cx1 + 1), iw)
    cy2 = min(max(y2 + my, cy1 + 1), ih)
    buf = io.BytesIO()
    img.crop((cx1, cy1, cx2, cy2)).save(buf, format="JPEG")
    return base64.b64encode(buf.getvalue()).decode()


def _cosine(a, b) -> float:
    dot = sum(x * y for x, y in zip(a, b))
    na = math.sqrt(sum(x * x for x in a))
    nb = math.sqrt(sum(y * y for y in b))
    return dot / (na * nb) if na and nb else 0.0


def _timed(fn, inp):
    t0 = time.perf_counter()
    out = fn(inp)
    return out, (time.perf_counter() - t0) * 1000.0


def main() -> int:
    meta_path = Path(sys.argv[1]) if len(sys.argv) > 1 else HERE / "face_frame_01.json"
    meta = json.loads(meta_path.read_text())
    base = meta_path.parent
    frame_path = base / meta["frame"]
    reference = json.loads((base / meta["reference"]["embedding_file"]).read_text())

    crop_b64 = _crop_b64(frame_path, meta["bbox"], meta.get("crop_margin_pct", 30))
    frame_b64 = _b64(frame_path)

    print(f"fixture      : {meta_path.name}  ({meta['source_face_id']})")
    print(f"frame        : {frame_path.name}  {meta['frame_size']}  bbox={meta['bbox']}")
    print(f"reference    : {meta['reference']['provider']}  (dim {len(reference)})")
    print("-" * 68)

    # 1) production path: detect+embed on the tight crop (best quality when SCRFD re-detects)
    out, ms = _timed(handler.face_detect_embed, {"image_b64": crop_b64})
    if out.get("face"):
        cos = _cosine(out["embedding"], reference)
        print(f"crop  face_detect_embed : FACE  det={out['det_score']:.3f}  "
              f"cos={cos:.4f}  {ms:.0f} ms")
    else:
        print(f"crop  face_detect_embed : NO FACE  {ms:.0f} ms   "
              f"(SCRFD missed it — the fix's fallback path)")
        # 2) fallback path: align-free embed of the same crop (embed_face_crop's fallback)
        out2, ms2 = _timed(handler.face_embed, {"image_b64": crop_b64})
        cos = _cosine(out2["embedding"], reference)
        print(f"crop  face_embed (align-free fallback): cos={cos:.4f}  {ms2:.0f} ms")

    # 3) sanity: detect+embed on the WHOLE frame (what the no-bbox path would do)
    out3, ms3 = _timed(handler.face_detect_embed, {"image_b64": frame_b64})
    if out3.get("face"):
        cos3 = _cosine(out3["embedding"], reference)
        print(f"frame face_detect_embed : FACE  det={out3['det_score']:.3f}  "
              f"cos={cos3:.4f}  {ms3:.0f} ms")
    else:
        print(f"frame face_detect_embed : NO FACE  {ms3:.0f} ms")

    print("-" * 68)
    print("cos ~1.0 = local sidecar matches the cloud reference; the same identity should")
    print("stay well under the 0.40 recognition distance (cos > 0.60) across providers.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
