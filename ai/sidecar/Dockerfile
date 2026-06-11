# Sentinel AI sidecar — GPU image for RunPod Serverless.
#
# Coherent CUDA stack: the base ships torch + CUDA 12.4 + cuDNN 9, and
# onnxruntime-gpu (PyPI default, 1.19+) targets exactly CUDA 12 / cuDNN 9 — so
# InsightFace's CUDAExecutionProvider lines up with no extra-index wheel dance.
# If a future bump breaks the GPU provider, the base CUDA/cuDNN majors and the
# onnxruntime-gpu pin (requirements.txt) are the two knobs to move together;
# InsightFace falls back to CPU if the provider can't load, so it won't hard-crash.
FROM pytorch/pytorch:2.4.1-cuda12.4-cudnn9-runtime

# Link the GHCR package to the repo.
LABEL org.opencontainers.image.source="https://github.com/jackesdavid/Sentinel"

# OpenCV runtime libs (+ build tools: InsightFace builds a small Cython extension).
RUN apt-get update \
    && apt-get install -y --no-install-recommends build-essential libgl1 libglib2.0-0 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Be patient/resilient on big wheel downloads.
ENV PIP_DEFAULT_TIMEOUT=120 PIP_RETRIES=5

# InsightFace needs cython to build its extension at install time.
RUN pip install --no-cache-dir cython

COPY requirements.txt .
# torch/torchvision come from the base image (CUDA build) — requirements.txt omits
# them so pip doesn't pull a CPU wheel over the top.
RUN pip install --no-cache-dir -r requirements.txt

# Bake model weights into the image so workers don't fetch them on the first request
# (fast cold starts): YOLOv8n + the InsightFace detection/recognition models
# (ctx_id=-1 downloads on CPU at build time — no GPU needed to bake them in).
RUN python -c "from ultralytics import YOLO; YOLO('yolov8n.pt')"
RUN python -c "import insightface; a=insightface.app.FaceAnalysis(name='buffalo_l', allowed_modules=['detection','recognition']); a.prepare(ctx_id=-1)"

COPY handler.py .

# RunPod serverless entrypoint (also serves /runsync locally with --rp_serve_api).
CMD ["python", "-u", "handler.py"]
