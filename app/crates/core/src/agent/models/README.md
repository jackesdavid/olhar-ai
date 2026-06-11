# Vendored model

`yolox_nano.onnx` — YOLOX-Nano, exported by Megvii (release 0.1.1rc0).
Source: https://github.com/Megvii-BaseDetection/YOLOX/releases/tag/0.1.1rc0
License: Apache-2.0 (see `LICENSE.yolox`).

- Input  `images`: `[1, 3, 416, 416]` float32, RGB, **raw 0–255** (no /255, no mean/std), letterboxed with pad value 114.
- Output `output`: `[1, 3549, 85]` — anchor-major, undecoded box geometry.
  Decode (see `detector.rs`): `xy=(raw+grid)*stride`, `wh=exp(raw)*stride` over strides 8/16/32;
  obj & cls are already sigmoid-activated in-graph; `score = obj * cls`.
