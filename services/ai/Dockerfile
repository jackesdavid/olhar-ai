# Sentinel AI sidecar — CPU image (works on RunPod CPU or low-end GPU endpoints).
# For a CUDA/GPU build, swap the base for e.g. `runpod/pytorch:*-cu121` and let
# ultralytics pick up the GPU automatically.
FROM python:3.11-slim

# Link the GHCR package to the repo.
LABEL org.opencontainers.image.source="https://github.com/jackesdavid/Sentinel"

# OpenCV (pulled in by ultralytics) needs these shared libs at runtime.
RUN apt-get update \
    && apt-get install -y --no-install-recommends libgl1 libglib2.0-0 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Be patient/resilient on big wheel downloads.
ENV PIP_DEFAULT_TIMEOUT=120 PIP_RETRIES=5

COPY requirements.txt .
# Install CPU-only torch FIRST so pip doesn't drag in the default CUDA build
# (~2.5 GB of nvidia_* wheels) — keeps the serverless image small and the build
# reliable. YOLOv8n runs fine on CPU. (For a GPU endpoint, base this image on
# `runpod/pytorch:*` which already ships a CUDA torch, and drop this line.)
RUN pip install --no-cache-dir torch torchvision --index-url https://download.pytorch.org/whl/cpu
RUN pip install --no-cache-dir -r requirements.txt

# Pre-download the YOLO weights into the image so workers don't fetch them on
# the first request (keeps cold starts fast).
RUN python -c "from ultralytics import YOLO; YOLO('yolov8n.pt')"

COPY handler.py .

# RunPod serverless entrypoint (also serves /runsync locally with --rp_serve_api).
CMD ["python", "-u", "handler.py"]
