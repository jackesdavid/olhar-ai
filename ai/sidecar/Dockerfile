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
LABEL org.opencontainers.image.source="https://github.com/olharai/Sentinel"

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

# CRITICAL: insightface hard-depends on the CPU `onnxruntime` wheel, which pip installs
# into the SAME `onnxruntime` namespace and shadows any onnxruntime-gpu — the result has
# NO CUDAExecutionProvider and silently runs on CPU (GPU prices, CPU speed). So after
# insightface is in, purge BOTH and install ONLY the GPU wheel, pinned to a version whose
# CUDA 12 / cuDNN 9 target matches the base image. The build-time assert fails hard if the
# CUDA provider isn't compiled in, so this regression can never ship unnoticed again.
RUN pip uninstall -y onnxruntime onnxruntime-gpu 2>/dev/null || true \
    && pip install --no-cache-dir "onnxruntime-gpu==1.20.1" \
    && python -c "import onnxruntime as ort; \
assert 'CUDAExecutionProvider' in ort.get_available_providers(), ort.get_available_providers(); \
print('onnxruntime', ort.__version__, 'CUDAExecutionProvider OK')"

# Bake model weights into the image so workers don't fetch them on the first request
# (fast cold starts): YOLOv8n + the InsightFace detection/recognition models
# (ctx_id=-1 downloads on CPU at build time — no GPU needed to bake them in).
# NOTE: this runs BEFORE the CUDA LD_LIBRARY_PATH below on purpose — putting the full CUDA
# stack on the path during the emulated (QEMU amd64) CPU bake segfaults it (exit 139).
RUN python -c "from ultralytics import YOLO; YOLO('yolov8n.pt')"
RUN python -c "import insightface; a=insightface.app.FaceAnalysis(name='buffalo_l', allowed_modules=['detection','recognition']); a.prepare(ctx_id=-1)"

# onnxruntime's CUDA EP dlopens the WHOLE nvidia CUDA stack at RUNTIME (cudnn, cublas,
# cusparse, cusolver, cufft, curand, nvrtc, cuda_runtime, …); the base image keeps them under
# torch's pip tree (/opt/conda/.../nvidia/*/lib), NOT on the default linker path. Missing even
# one (e.g. cusparse) makes the provider fail to load and silently fall back to CPU — so list
# ALL of them. Set LAST so it only affects the runtime CMD, not the build-time CPU bake above.
# RunPod injects the GPU driver at /usr/local/nvidia/lib (already on the path).
ENV LD_LIBRARY_PATH=/opt/conda/lib/python3.11/site-packages/nvidia/cublas/lib:/opt/conda/lib/python3.11/site-packages/nvidia/cuda_cupti/lib:/opt/conda/lib/python3.11/site-packages/nvidia/cuda_nvrtc/lib:/opt/conda/lib/python3.11/site-packages/nvidia/cuda_runtime/lib:/opt/conda/lib/python3.11/site-packages/nvidia/cudnn/lib:/opt/conda/lib/python3.11/site-packages/nvidia/cufft/lib:/opt/conda/lib/python3.11/site-packages/nvidia/curand/lib:/opt/conda/lib/python3.11/site-packages/nvidia/cusolver/lib:/opt/conda/lib/python3.11/site-packages/nvidia/cusparse/lib:/opt/conda/lib/python3.11/site-packages/nvidia/nccl/lib:/opt/conda/lib/python3.11/site-packages/nvidia/nvjitlink/lib:/opt/conda/lib/python3.11/site-packages/nvidia/nvtx/lib:${LD_LIBRARY_PATH}

COPY handler.py .

# RunPod serverless entrypoint (also serves /runsync locally with --rp_serve_api).
CMD ["python", "-u", "handler.py"]
