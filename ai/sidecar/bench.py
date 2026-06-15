#!/usr/bin/env python3
"""Latency benchmark for the Sentinel AI sidecar — compare backends (CPU VPS vs RTX 2060).

Stdlib only: no insightface/numpy needed, so it runs from anywhere (your Mac, the VPS,
a tailnet host) by just pointing it at a sidecar's /runsync URL. It posts a fixed,
checked-in test face (testdata/face_test.jpg — a synthetic, license-clean face) so the
two runs are directly comparable, and reports both the server compute time (`took_ms`,
the apples-to-apples GPU-vs-CPU number) and client wall-clock (includes network).

Usage:
  # CPU sidecar on the VPS (run inside the container — it isn't published to the host):
  ssh Contabo 'docker exec -i sentinel-ai python /dev/stdin' < ai/sidecar/bench.py
  # …or against the RTX 2060 box later, from anywhere on the tailnet:
  AI_SERVICE_URL=http://<rtx-box>:8000/runsync python ai/sidecar/bench.py --n 50
  # other ops / image:
  python ai/sidecar/bench.py --op face_detect --image some_face.jpg

Baselines — face_detect_embed, the testdata face (record new numbers as you measure):
  InsightFace CPU  (VPS sentinel-ai, in-container)  server p50 ~409 ms, wall ~414 ms      (2026-06-15)
  dlib/face_recog  CPU (VPS sentinel-ai-dlib, HOG)  server p50 ~552 ms, wall ~562 ms, 128-d (2026-06-15)
  InsightFace GPU  (RunPod serverless, from a Mac)  server p50 ~250 ms, wall ~1454 ms, cold-start ~74 s (2026-06-15)
  InsightFace GPU  (RTX 2060, on tailnet)           TBD  ← expected: GPU compute + low LAN latency, no cold start
"""
import argparse
import base64
import json
import os
import statistics
import time
import urllib.request

DEFAULT_IMAGE = os.path.join(os.path.dirname(os.path.abspath(__file__)), "testdata", "face_test.jpg")


def call(url, key, op, b64, timeout=180):
    """POST one op; return (wall_ms, output_dict)."""
    data = json.dumps({"input": {"op": op, "image_b64": b64}}).encode()
    req = urllib.request.Request(url, data=data, headers={"Content-Type": "application/json"})
    if key:
        req.add_header("Authorization", f"Bearer {key}")
    t0 = time.perf_counter()
    body = urllib.request.urlopen(req, timeout=timeout).read()
    wall = (time.perf_counter() - t0) * 1000.0
    r = json.loads(body)
    return wall, r.get("output", r)


def pctl(xs, p):
    xs = sorted(xs)
    k = (len(xs) - 1) * p / 100.0
    f = int(k)
    c = min(f + 1, len(xs) - 1)
    return xs[f] + (xs[c] - xs[f]) * (k - f)


def stats(xs):
    return (
        f"min={min(xs):.0f} p50={pctl(xs, 50):.0f} avg={statistics.mean(xs):.0f} "
        f"p95={pctl(xs, 95):.0f} max={max(xs):.0f} ms"
    )


def main():
    ap = argparse.ArgumentParser(description="Benchmark the Sentinel AI sidecar.")
    ap.add_argument("--url", default=os.environ.get("AI_SERVICE_URL", "http://127.0.0.1:8000/runsync"))
    ap.add_argument("--key", default=os.environ.get("AI_SERVICE_KEY", ""))
    ap.add_argument("--image", default=DEFAULT_IMAGE)
    ap.add_argument("--op", default="face_detect_embed",
                    help="face_detect_embed (default) | face_detect | face_embed | detect_person")
    ap.add_argument("--n", type=int, default=30, help="measured iterations")
    ap.add_argument("--warmup", type=int, default=3)
    a = ap.parse_args()

    with open(a.image, "rb") as fh:
        b64 = base64.b64encode(fh.read()).decode()
    print(f"sidecar : {a.url}")
    print(f"image   : {a.image} ({len(b64) * 3 // 4} bytes)  op={a.op}  n={a.n} (+{a.warmup} warmup)")

    # First call also covers any cold start (lazy model load); warmups settle the JIT/caches.
    first_wall, first = call(a.url, a.key, a.op, b64)
    print(f"first call: {first_wall:.0f} ms  "
          f"face/count={first.get('face', first.get('count'))} "
          f"det_score={round(first.get('det_score', 0), 3)} "
          f"embedding_dim={len(first.get('embedding', []))}")
    for _ in range(max(0, a.warmup - 1)):
        call(a.url, a.key, a.op, b64)

    walls, servers = [], []
    for _ in range(a.n):
        w, o = call(a.url, a.key, a.op, b64)
        walls.append(w)
        if o.get("took_ms") is not None:
            servers.append(o["took_ms"])

    print(f"server took_ms : {stats(servers)}" if servers else "server took_ms : (not reported)")
    print(f"client wall    : {stats(walls)}")


if __name__ == "__main__":
    main()
